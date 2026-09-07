package io.multiai.cli.converge;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;

/**
 * 교차검토 응답 스키마. 세 참여자에게 동일하게 강제한다. SPEC §7.9 Phase 3.
 *
 * 세 CLI 모두 스키마 강제가 가능하다 (§1.5 실측):
 *   claude --json-schema <스키마>    / --output-format json
 *   codex  --output-schema <FILE>    / -o <FILE>
 *   agy    --json-schema <경로>      / --output-format json → structured_output
 *
 * 스키마는 **반드시 파일 경로로** 전달한다. Windows 에서 인라인 JSON 문자열을
 * 인자로 넘기면 따옴표 이스케이프가 깨진다 (§1.5 실측).
 *
 * 모든 object 에 "additionalProperties": false 가 필요하다 — codex 는 OpenAI strict
 * 구조화 출력을 쓰므로 이것이 없으면 400 invalid_json_schema 로 거부한다 (실측).
 */
public final class ReviewSchema {

    public static final String SCHEMA = """
            {
              "type": "object",
              "additionalProperties": false,
              "required": ["verdict", "summary", "issues", "open_questions"],
              "properties": {
                "verdict": { "type": "string", "enum": ["AGREE", "CONCERNS", "BLOCK"] },
                "summary": { "type": "string" },
                "issues": {
                  "type": "array",
                  "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "severity", "claim", "rationale", "suggestion"],
                    "properties": {
                      "id":         { "type": "string" },
                      "severity":   { "type": "string", "enum": ["critical", "major", "minor"] },
                      "claim":      { "type": "string" },
                      "rationale":  { "type": "string" },
                      "suggestion": { "type": "string" }
                    }
                  }
                },
                "open_questions": { "type": "array", "items": { "type": "string" } }
              }
            }
            """;

    private ReviewSchema() {}

    /**
     * 개행을 제거한 한 줄 스키마.
     *
     * claude 는 --json-schema 에 **스키마 문자열**을 받는데, 여러 줄 문자열을 인자로
     * 넘기면 "--json-schema is not valid JSON" 으로 거부한다 (실측). 압축하면 통과한다.
     */
    public static String minified() {
        StringBuilder b = new StringBuilder(SCHEMA.length());
        boolean inStr = false;
        for (int i = 0; i < SCHEMA.length(); i++) {
            char c = SCHEMA.charAt(i);
            if (c == '"' && (i == 0 || SCHEMA.charAt(i - 1) != '\\')) inStr = !inStr;
            if (!inStr && Character.isWhitespace(c)) continue;
            b.append(c);
        }
        return b.toString();
    }

    /**
     * 프롬프트에 스키마를 직접 넣는다.
     *
     * CLI 레벨 스키마 강제가 항상 되는 것은 아니다 — codex 는 ChatGPT 계정에서
     * 'model is not supported' 로 --output-schema 를 거부한다 (실측). 프롬프트에
     * 스키마를 넣어두면 CLI 강제가 없어도 형식을 맞출 수 있다.
     */
    private static String schemaBlock() {
        return """

               ## 응답 형식 — 이 JSON 스키마를 정확히 따르라

               ```json
               %s
               ```

               **JSON 객체 하나만 출력하라.** 설명, 인사말, 코드펜스 밖의 문장을 쓰지 마라.
               """.formatted(SCHEMA.strip());
    }

    /** 스키마를 파일로 떨어뜨린다. 워크스페이스가 아니라 앱 temp 에 만든다 (§7.6). */
    public static Path writeTo(Path tempDir) throws IOException {
        Files.createDirectories(tempDir);
        Path f = tempDir.resolve("review-schema.json");
        Files.writeString(f, SCHEMA, StandardCharsets.UTF_8);
        return f;
    }

    /** 1라운드 검토 요청 프롬프트. */
    public static String round1Prompt(String subject) {
        return """
               너는 독립 검토자다. 아래 안건을 검토하고 **지정된 JSON 스키마에 맞춰서만** 답하라.

               - 동의를 위한 동의를 하지 마라. 근거 없는 지적도 하지 마라.
               - `verdict` 는 AGREE(진행 가능) / CONCERNS(보완 필요) / BLOCK(이대로 진행 불가) 중 하나다.
               - 각 issue 의 `id` 는 너의 응답 안에서 고유해야 한다.
               - 판단에 필요한 정보가 없으면 지어내지 말고 `open_questions` 에 적어라.
               - 파일을 수정하지 마라. 검토만 한다.

               ## 안건

               %s
               %s
               """.formatted(subject, schemaBlock());
    }

    /** 2라운드 반론 프롬프트. 상대 의견을 첨부하되 출처는 밝히지 않는다. */
    public static String round2Prompt(String subject, String opposing) {
        return """
               너는 독립 검토자다. **2라운드**다. 아래는 같은 안건을 검토한 다른 AI 의
               의견이다. 누가 말했는지는 알려주지 않는다. 출처가 아니라 논지로 판단하라.

               읽고 나서 **네 입장을 유지할지 철회할지** 밝히고, 같은 JSON 스키마로 답하라.

               - 유지하려면 상대 논지의 **어디가 왜 틀렸는지** 짚어라. 못 짚으면 유지가 아니다.
               - 틀렸으면 철회하라. 여기서 철회하는 것에는 아무 불이익이 없다.
               - 양보를 위한 양보도, 버티기 위한 버티기도 하지 마라.
               - 새 쟁점을 꺼내지 마라. 아래 안건과 상대 의견에 한정한다.

               ## 안건

               %s

               ## 다른 검토자의 의견

               %s
               %s
               """.formatted(subject, opposing, schemaBlock());
    }
}
