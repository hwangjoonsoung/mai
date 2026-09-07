package io.multiai.cli.app;

import java.util.*;

/**
 * 입력 문법 파싱. SPEC §5.2.
 *
 * 일반 문장          → 전 참여자 동시 호출
 * @all <질문>        → 동시 호출 명시
 * @claude/@codex/@gemini <질문> → 지목 (복수 지목 가능)
 * /status /new /rooms /open /cancel /exit
 *
 * Phase 1 명령: 일반 채팅, 멘션, /status, /new, /exit, /cancel, /rooms, /open.
 * /run --write 와 프리셋은 Phase 2 다.
 */
public final class CommandParser {

    public sealed interface Input permits Chat, Slash {}

    /** @param targets 비어 있으면 전 참여자 */
    public record Chat(List<String> targets, String text) implements Input {}

    /**
     * @param args 공백으로 자른 인수
     * @param raw  명령 이름 뒤의 원문. 프롬프트처럼 공백을 보존해야 할 때 쓴다.
     */
    public record Slash(String name, List<String> args, String raw) implements Input {}

    private final Set<String> knownMentions;

    public CommandParser(Set<String> providerIds) {
        this.knownMentions = new LinkedHashSet<>(providerIds);
    }

    public Input parse(String raw) {
        String s = raw.strip();
        if (s.startsWith("/")) {
            String[] parts = s.substring(1).split("[ \\t]+");
            String name = parts.length > 0 ? parts[0].toLowerCase(Locale.ROOT) : "";
            List<String> args = parts.length > 1
                    ? List.of(Arrays.copyOfRange(parts, 1, parts.length))
                    : List.of();
            String body = s.substring(1);
            int sp = firstSpace(body);
            String tail = sp < 0 ? "" : body.substring(sp + 1).stripLeading();
            return new Slash(name, args, tail);
        }

        List<String> targets = new ArrayList<>();
        String rest = s;
        while (rest.startsWith("@")) {
            int sp = firstSpace(rest);
            String tok = (sp < 0 ? rest : rest.substring(0, sp)).substring(1).toLowerCase(Locale.ROOT);
            if (!tok.equals("all") && !knownMentions.contains(tok)) break;
            if (!tok.equals("all") && !targets.contains(tok)) targets.add(tok);
            rest = sp < 0 ? "" : rest.substring(sp + 1).stripLeading();
        }
        return new Chat(targets, rest);
    }

    private static int firstSpace(String s) {
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == ' ' || c == '\t') return i;
        }
        return -1;
    }
}
