package io.multiai.cli.converge;

import io.multiai.cli.orchestration.ParallelRoundExecutor;
import io.multiai.cli.provider.*;

import java.io.IOException;
import java.nio.file.Path;
import java.time.Duration;
import java.util.*;

/**
 * 구조화 수렴 한 건. SPEC §7.5-1 · §7.9 Phase 3.
 *
 * 흐름:
 *   1라운드 — 검토자들이 서로 못 본 상태에서 동일 스키마로 독립 답변
 *   분류    — 합의 / 이견 / 단독 지적 / 미해결
 *   2라운드 — 이견 또는 critical·major 단독 지적이 있을 때만. 최대 1회
 *   보고서  — REPORT.md
 *
 * **수렴자로 지명된 참여자는 검토 대상에서 제외된다** (§7.5-1). 지명이 없으면
 * 규칙 기반 분류만 수행한다 — 모델 호출 없이 스키마 필드만 대조한다.
 */
public final class ConvergeSession {

    /** 진행 상황을 화면에 알리는 콜백. */
    public interface Progress {
        void stage(String message);
        void reviewerDone(StructuredReview r);
    }

    private final ParallelRoundExecutor executor;
    private final Path tempDir;
    private final Duration timeout;

    public ConvergeSession(ParallelRoundExecutor executor, Path tempDir, Duration timeout) {
        this.executor = executor;
        this.tempDir = tempDir;
        this.timeout = timeout;
    }

    public record Result(ConsolidationEngine.Outcome round1,
                         ConsolidationEngine.Outcome round2,
                         Path report,
                         String abortReason) {
        public boolean aborted() {
            return abortReason != null;
        }
    }

    /**
     * @param reviewers 검토자. 수렴자로 지명된 참여자는 호출부에서 이미 제외돼 있다.
     * @param subject   안건
     * @param outDir    REPORT.md 를 쓸 디렉터리
     */
    public Result run(List<AiProvider> reviewers, String subject, Path workspace,
                      Path outDir, Progress progress) throws IOException {

        if (reviewers.size() < 2) {
            return new Result(null, null, null,
                    "검토자가 2명 미만이다. 수렴에는 최소 2명이 필요하다.");
        }
        Path schema = ReviewSchema.writeTo(tempDir);

        // ---- 1라운드: 독립 검토 ----
        progress.stage("1라운드 — " + reviewers.size() + "명 독립 검토");
        ConsolidationEngine.Outcome r1 =
                round(reviewers, ReviewSchema.round1Prompt(subject), workspace,
                        outDir.resolve("r1"), schema, progress);

        if (r1.reviews().stream().noneMatch(StructuredReview::valid)) {
            // 전원 실패면 중단한다. 분류를 지어내지 않는다 (§7.5-1 실패 처리).
            return new Result(r1, null, null,
                    "전원 응답 실패 — 인증·쿼터·네트워크를 확인하라.");
        }
        if (r1.unanimousAgree()) {
            progress.stage("전원 AGREE 이고 지적이 없다 — 즉시 종료 (2라운드 없음)");
            Path rep = ReportWriter.write(outDir, subject, r1, null);
            return new Result(r1, null, rep, null);
        }

        // ---- 2라운드: 상호 반론 (조건부, 최대 1회) ----
        ConsolidationEngine.Outcome r2 = null;
        if (r1.needsRound2()) {
            progress.stage("2라운드 — " + r1.round2Reason());
            r2 = rebuttal(reviewers, subject, r1, workspace, outDir.resolve("r2"), schema, progress);
        } else {
            progress.stage("2라운드 조건 미충족 — 1라운드로 종료");
        }

        Path rep = ReportWriter.write(outDir, subject, r1, r2);
        return new Result(r1, r2, rep, null);
    }

    /** 한 라운드를 실행하고 스키마 위반 시 1회 재시도한다 (§7.5-1 실패 처리). */
    private ConsolidationEngine.Outcome round(List<AiProvider> targets, String prompt,
                                              Path workspace, Path runDir, Path schema,
                                              Progress progress) throws IOException {
        java.nio.file.Files.createDirectories(runDir);
        List<ProviderResult> raw = executor.run(targets, prompt, workspace, false,
                runDir, tempDir, timeout, schema, r -> {});

        List<StructuredReview> reviews = new ArrayList<>();
        List<String> failed = new ArrayList<>();
        for (ProviderResult pr : raw) {
            StructuredReview sr = toReview(pr);
            if (!sr.valid()) {
                // 스키마 위반·파싱 실패는 해당 참여자만 1회 재시도한다.
                progress.stage(pr.displayName() + " 응답이 스키마를 벗어났다 — 1회 재시도");
                AiProvider one = find(targets, pr.providerId());
                if (one != null) {
                    List<ProviderResult> retry = executor.run(List.of(one), prompt, workspace,
                            false, runDir, tempDir, timeout, schema, r -> {});
                    if (!retry.isEmpty()) sr = toReview(retry.get(0));
                }
            }
            if (sr.valid()) {
                progress.reviewerDone(sr);
            } else {
                failed.add(sr.reviewerName() + " (" + sr.parseNote() + ")");
            }
            reviews.add(sr);
        }
        return ConsolidationEngine.consolidate(reviews, failed);
    }

    /** 2라운드. 각 검토자에게 **상대의 의견만** 첨부한다 — 비대칭 문맥이다. */
    private ConsolidationEngine.Outcome rebuttal(List<AiProvider> reviewers, String subject,
                                                 ConsolidationEngine.Outcome r1, Path workspace,
                                                 Path runDir, Path schema, Progress progress)
            throws IOException {
        java.nio.file.Files.createDirectories(runDir);
        List<StructuredReview> out = new ArrayList<>();
        List<String> failed = new ArrayList<>();

        for (AiProvider p : reviewers) {
            String opposing = renderOthers(r1, p.id());
            if (opposing.isBlank()) continue;
            List<ProviderResult> res = executor.run(List.of(p),
                    ReviewSchema.round2Prompt(subject, opposing), workspace, false,
                    runDir, tempDir, timeout, schema, r -> {});
            if (res.isEmpty()) {
                failed.add(p.displayName() + " (응답 없음)");
                continue;
            }
            StructuredReview sr = toReview(res.get(0));
            if (sr.valid()) progress.reviewerDone(sr);
            else failed.add(sr.reviewerName() + " (" + sr.parseNote() + ")");
            out.add(sr);
        }
        return ConsolidationEngine.consolidate(out, failed);
    }

    /** 자기 자신을 뺀 나머지 검토자의 의견을 사람이 읽을 형태로 만든다. */
    private static String renderOthers(ConsolidationEngine.Outcome r1, String selfId) {
        StringBuilder b = new StringBuilder();
        for (StructuredReview r : r1.reviews()) {
            if (r.reviewerId().equals(selfId) || !r.valid()) continue;
            b.append("- 판정: ").append(r.verdict()).append('\n');
            b.append("- 요약: ").append(r.summary()).append('\n');
            for (StructuredReview.Issue is : r.issues()) {
                b.append("- [").append(is.severity().name().toLowerCase(Locale.ROOT)).append("] ")
                 .append(is.claim()).append("\n  근거: ").append(is.rationale()).append('\n');
            }
            b.append('\n');
        }
        return b.toString().strip();
    }

    private static StructuredReview toReview(ProviderResult pr) {
        if (!pr.ok()) {
            return StructuredReview.parse(pr.providerId(), pr.displayName(), "");
        }
        return StructuredReview.parse(pr.providerId(), pr.displayName(), pr.text());
    }

    private static AiProvider find(List<AiProvider> ps, String id) {
        for (AiProvider p : ps) if (p.id().equals(id)) return p;
        return null;
    }
}
