package io.multiai.cli.converge;

import io.multiai.cli.converge.StructuredReview.*;

import java.util.*;

/**
 * 분류와 수렴 판정. SPEC §7.5-1 · §7.9.
 *
 * **수렴자는 판정하지 않는다.** 두 의견을 분류하고 쟁점을 좁혀 사용자 앞에 놓는
 * 것까지가 역할이다. 어느 쪽이 옳은지 단정하면 3자 구도의 의미가 사라진다.
 * **기각한 지적도 이유와 함께 남긴다** — 조용히 빠뜨리면 다음 라운드에 다시 올라온다.
 */
public final class ConsolidationEngine {

    public enum Bucket { 합의, 이견, 단독지적, 미해결 }

    /**
     * @param bucket   분류
     * @param claim    쟁점 요약
     * @param severity 최고 심각도
     * @param byWhom   이 쟁점을 제기한 검토자
     * @param details  검토자별 원문 (id -> issue)
     */
    public record Finding(Bucket bucket, String claim, Severity severity,
                          List<String> byWhom, Map<String, Issue> details) {}

    public record Outcome(
            List<StructuredReview> reviews,
            List<Finding> findings,
            List<String> openQuestions,
            boolean needsRound2,
            String round2Reason,
            List<String> failedReviewers) {

        public boolean partial() {
            return !failedReviewers.isEmpty();
        }

        /** 전원 AGREE 이고 issue 가 없으면 즉시 종료한다 (§7.5-1). */
        public boolean unanimousAgree() {
            return !reviews.isEmpty()
                    && reviews.stream().allMatch(r -> r.verdict() == Verdict.AGREE)
                    && reviews.stream().allMatch(r -> r.issues().isEmpty());
        }
    }

    private ConsolidationEngine() {}

    public static Outcome consolidate(List<StructuredReview> all, List<String> failed) {
        List<StructuredReview> valid = all.stream().filter(StructuredReview::valid).toList();

        // 같은 쟁점을 claim 의 정규화 키로 묶는다. 완전 자동 매칭은 불가능하므로
        // 겹치지 않으면 단독 지적으로 남기고 사용자가 판단하게 둔다.
        Map<String, List<StructuredReview>> raisedBy = new LinkedHashMap<>();
        Map<String, Map<String, Issue>> detailsOf = new LinkedHashMap<>();
        Map<String, Severity> worst = new LinkedHashMap<>();
        Map<String, String> label = new LinkedHashMap<>();

        for (StructuredReview r : valid) {
            for (Issue is : r.issues()) {
                String key = normalize(is.claim());
                raisedBy.computeIfAbsent(key, k -> new ArrayList<>()).add(r);
                detailsOf.computeIfAbsent(key, k -> new LinkedHashMap<>())
                        .put(r.reviewerId(), is);
                label.putIfAbsent(key, is.claim());
                worst.merge(key, is.severity(),
                        (a, b) -> a.ordinal() <= b.ordinal() ? a : b);
            }
        }

        List<Finding> findings = new ArrayList<>();
        for (String key : raisedBy.keySet()) {
            List<StructuredReview> who = raisedBy.get(key);
            Bucket bucket = who.size() >= 2 ? Bucket.합의 : Bucket.단독지적;
            findings.add(new Finding(bucket, label.get(key), worst.get(key),
                    who.stream().map(StructuredReview::reviewerId).toList(),
                    detailsOf.get(key)));
        }

        // verdict 가 갈리면 그 자체를 최우선 이견으로 올린다 (§R2 판정 충돌 규칙).
        Set<Verdict> verdicts = new LinkedHashSet<>();
        valid.forEach(r -> verdicts.add(r.verdict()));
        if (verdicts.size() > 1) {
            Map<String, Issue> d = new LinkedHashMap<>();
            for (StructuredReview r : valid) {
                d.put(r.reviewerId(), new Issue("verdict", Severity.MAJOR,
                        "판정 " + r.verdict(), r.summary(), ""));
            }
            findings.add(0, new Finding(Bucket.이견, "판정이 갈렸다: " + verdicts,
                    Severity.MAJOR, valid.stream().map(StructuredReview::reviewerId).toList(), d));
        }

        List<String> oq = new ArrayList<>();
        for (StructuredReview r : valid) {
            for (String q : r.openQuestions()) oq.add(r.reviewerName() + ": " + q);
        }

        // 2라운드 조건 — 이견, 또는 critical·major 단독 지적 (§7.5-1)
        boolean disagreement = findings.stream().anyMatch(f -> f.bucket() == Bucket.이견);
        boolean majorSolo = findings.stream()
                .anyMatch(f -> f.bucket() == Bucket.단독지적 && f.severity().atLeastMajor());
        String reason = disagreement ? "판정 또는 쟁점에 이견이 있다"
                : majorSolo ? "critical·major 단독 지적이 있다" : "";
        boolean needs2 = (disagreement || majorSolo) && valid.size() >= 2;

        return new Outcome(all, findings, oq, needs2, reason, failed);
    }

    /** claim 매칭용 정규화. 공백·구두점·대소문자를 무시한다. */
    private static String normalize(String s) {
        if (s == null) return "";
        StringBuilder sb = new StringBuilder();
        for (char c : s.toLowerCase(Locale.ROOT).toCharArray()) {
            if (Character.isLetterOrDigit(c)) sb.append(c);
        }
        String t = sb.toString();
        return t.length() > 60 ? t.substring(0, 60) : t;
    }
}
