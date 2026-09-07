package io.multiai.cli.converge;

import io.multiai.cli.converge.ConsolidationEngine.*;
import io.multiai.cli.converge.StructuredReview.*;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.time.LocalDate;
import java.util.*;

/**
 * REPORT.md 생성. SPEC §7.9.
 *
 * 형식의 참조 구현은 수동 예행연습 산출물인 REVIEW_REPORT.md 다.
 * 거기서 얻은 두 규칙을 유지한다.
 *   1. 기각한 지적도 이유와 함께 남긴다.
 *   2. 수렴자는 판정하지 않는다. 분류해서 사용자 앞에 놓는 것까지가 역할이다.
 */
public final class ReportWriter {

    private ReportWriter() {}

    public static Path write(Path dir, String subject, Outcome r1, Outcome r2) throws IOException {
        Files.createDirectories(dir);
        Path f = dir.resolve("REPORT.md");
        Files.writeString(f, render(subject, r1, r2), StandardCharsets.UTF_8);
        return f;
    }

    public static String render(String subject, Outcome r1, Outcome r2) {
        StringBuilder b = new StringBuilder();
        Outcome last = r2 != null ? r2 : r1;

        b.append("# REPORT — 교차검토 수렴\n\n");
        b.append("- 일시: ").append(LocalDate.now()).append('\n');
        b.append("- 라운드: ").append(r2 != null ? 2 : 1).append('\n');
        b.append("- 검토자: ").append(names(r1)).append('\n');
        // 두 라운드의 실패를 합쳐 알린다 — r1 만 보면 2라운드 실패를 놓친다.
        Set<String> failed = new LinkedHashSet<>(r1.failedReviewers());
        if (r2 != null) failed.addAll(r2.failedReviewers());
        if (!failed.isEmpty()) {
            b.append("- **PARTIAL — 응답 없음: ").append(String.join(", ", failed))
             .append("**\n");
        }
        b.append('\n');

        b.append("## 0. 판정 요약\n\n");
        b.append("| 검토자 | verdict | 핵심 우려 |\n|---|:---:|---|\n");
        for (StructuredReview r : last.reviews()) {
            b.append("| ").append(r.reviewerName()).append(" | ")
             .append(r.valid() ? r.verdict() : "응답 없음").append(" | ")
             .append(oneLine(r.valid() ? r.summary() : r.parseNote())).append(" |\n");
        }
        b.append('\n');

        b.append("## 1. 안건\n\n").append(subject).append("\n\n");

        section(b, "2. 합의 — 그대로 반영", last, Bucket.합의);
        section(b, "3. 이견 — 판단 상충", last, Bucket.이견);
        section(b, "4. 단독 지적", last, Bucket.단독지적);

        b.append("## 5. 미해결 — 사용자 결정 필요\n\n");
        List<String> unresolved = new ArrayList<>(last.openQuestions());
        if (r2 != null) {
            for (Finding f : r2.findings()) {
                if (f.bucket() == Bucket.이견) {
                    unresolved.add("2라운드 후에도 좁혀지지 않음: " + f.claim());
                }
            }
        } else if (r1.needsRound2()) {
            unresolved.add("2라운드가 필요하나 실행되지 않았다 — " + r1.round2Reason());
        }
        if (unresolved.isEmpty()) {
            b.append("없음.\n\n");
        } else {
            for (String q : unresolved) b.append("- ").append(q).append('\n');
            b.append('\n');
        }

        if (r2 != null) {
            b.append("## 6. 2라운드 반론 결과\n\n");
            b.append("사유: ").append(r1.round2Reason()).append("\n\n");
            for (StructuredReview r : r2.reviews()) {
                b.append("### ").append(r.reviewerName()).append(" — ")
                 .append(r.valid() ? r.verdict().toString() : "응답 없음").append("\n\n");
                if (r.valid()) b.append(r.summary()).append("\n\n");
            }
        }

        b.append("---\n\n");
        b.append("> 이 보고서는 분류만 한다. **어느 쪽이 옳은지 판정하지 않는다.**\n");
        b.append("> 미해결 항목은 사용자가 결정한다. 최대 2라운드까지만 반론한다.\n");
        return b.toString();
    }

    private static void section(StringBuilder b, String title, Outcome o, Bucket bucket) {
        b.append("## ").append(title).append("\n\n");
        List<Finding> fs = o.findings().stream().filter(f -> f.bucket() == bucket)
                .sorted(Comparator.comparingInt(f -> f.severity().ordinal())).toList();
        if (fs.isEmpty()) {
            b.append("없음.\n\n");
            return;
        }
        for (Finding f : fs) {
            b.append("### [").append(f.severity().name().toLowerCase(Locale.ROOT)).append("] ")
             .append(f.claim()).append("\n\n");
            b.append("- 제기: ").append(String.join(", ", f.byWhom())).append('\n');
            for (Map.Entry<String, Issue> e : f.details().entrySet()) {
                Issue is = e.getValue();
                b.append("- **").append(e.getKey()).append("** — ").append(oneLine(is.rationale()));
                if (!is.suggestion().isBlank()) {
                    b.append("\n  - 제안: ").append(oneLine(is.suggestion()));
                }
                b.append('\n');
            }
            b.append('\n');
        }
    }

    private static String names(Outcome o) {
        return String.join(", ", o.reviews().stream().map(StructuredReview::reviewerName).toList());
    }

    private static String oneLine(String s) {
        return s == null ? "" : s.replace('\n', ' ').replace('|', '/').strip();
    }
}
