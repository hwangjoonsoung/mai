package io.multiai.cli.converge;

import java.util.*;

/**
 * 스키마를 통과한 검토자 응답 하나. SPEC §7.9.
 */
public record StructuredReview(
        String reviewerId,
        String reviewerName,
        Verdict verdict,
        String summary,
        List<Issue> issues,
        List<String> openQuestions,
        String parseNote) {

    public enum Verdict { AGREE, CONCERNS, BLOCK, UNKNOWN }

    public enum Severity { CRITICAL, MAJOR, MINOR;
        static Severity of(String s) {
            if (s == null) return MINOR;
            return switch (s.toLowerCase(Locale.ROOT)) {
                case "critical" -> CRITICAL;
                case "major" -> MAJOR;
                default -> MINOR;
            };
        }
        public boolean atLeastMajor() { return this == CRITICAL || this == MAJOR; }
    }

    public record Issue(String id, Severity severity, String claim,
                        String rationale, String suggestion) {}

    public boolean valid() {
        return verdict != Verdict.UNKNOWN;
    }

    /**
     * 공급자 원시 출력에서 구조화 응답을 뽑는다.
     *
     * agy 는 --output-format json 의 최상위에 structured_output 을 담는다(§1.5).
     * codex 는 -o 파일에 최종 메시지(=JSON)를 쓴다. claude 는 JSON 을 출력한다.
     * 셋 다 앞뒤에 배너가 붙을 수 있으므로 텍스트에서 객체를 찾아 파싱한다.
     */
    public static StructuredReview parse(String reviewerId, String reviewerName, String raw) {
        Optional<Map<String, Object>> found = Json.findObject(raw);
        if (found.isEmpty()) {
            return invalid(reviewerId, reviewerName, "JSON 객체를 찾지 못했다");
        }
        Map<String, Object> root = found.get();

        // agy 래퍼 대응 — structured_output 이 있으면 그 안이 본체다.
        Map<String, Object> body = root.containsKey("structured_output")
                ? Json.map(root.get("structured_output")) : root;

        String v = Json.str(body, "verdict", "");
        Verdict verdict;
        try {
            verdict = Verdict.valueOf(v.toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException e) {
            return invalid(reviewerId, reviewerName, "verdict 가 스키마 위반: '" + v + "'");
        }

        List<Issue> issues = new ArrayList<>();
        for (Object o : Json.list(body, "issues")) {
            Map<String, Object> m = Json.map(o);
            issues.add(new Issue(
                    Json.str(m, "id", "?"),
                    Severity.of(Json.str(m, "severity", "minor")),
                    Json.str(m, "claim", ""),
                    Json.str(m, "rationale", ""),
                    Json.str(m, "suggestion", "")));
        }
        List<String> oq = new ArrayList<>();
        for (Object o : Json.list(body, "open_questions")) {
            if (o instanceof String s && !s.isBlank()) oq.add(s);
        }
        return new StructuredReview(reviewerId, reviewerName, verdict,
                Json.str(body, "summary", ""), issues, oq, "");
    }

    private static StructuredReview invalid(String id, String name, String note) {
        return new StructuredReview(id, name, Verdict.UNKNOWN, "",
                List.of(), List.of(), note);
    }
}
