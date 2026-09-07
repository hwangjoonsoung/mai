package io.multiai.cli.app;

import java.util.*;

/**
 * /run <멘션> [--write] <프롬프트> 파싱. SPEC §5.2 · §7.5 「쓰기 실행」.
 *
 * - 쓰기 프로필은 이 호출 **한 번에만** 적용된다. 세션에 남지 않는다.
 * - 같은 워크스페이스에 여러 AI 를 동시에 쓰기 모드로 실행하지 않는다 —
 *   --write 는 참여자를 정확히 하나만 지목해야 한다.
 */
public record RunCommand(List<String> targets, boolean write, String prompt) {

    public static final class ParseException extends Exception {
        public ParseException(String m) { super(m); }
    }

    private static final String USAGE =
            "사용법: /run @<참여자> [--write] <프롬프트>";

    public static RunCommand parse(List<String> args, String raw, Set<String> knownIds)
            throws ParseException {
        List<String> targets = new ArrayList<>();
        boolean write = false;
        int i = 0;

        while (i < args.size() && args.get(i).startsWith("@")) {
            String id = args.get(i).substring(1).toLowerCase(Locale.ROOT);
            if (!knownIds.contains(id)) {
                throw new ParseException("알 수 없는 참여자: @" + id);
            }
            if (!targets.contains(id)) targets.add(id);
            i++;
        }
        while (i < args.size() && args.get(i).startsWith("--")) {
            if (args.get(i).equals("--write")) {
                write = true;
            } else {
                throw new ParseException("알 수 없는 옵션: " + args.get(i) + "\n" + USAGE);
            }
            i++;
        }
        if (targets.isEmpty()) {
            throw new ParseException("참여자를 지목해야 한다.\n" + USAGE);
        }
        // SPEC §7.5 — 병렬 구현이 필요하면 사용자가 서로 다른 워크스페이스를 명시해야 한다.
        if (write && targets.size() > 1) {
            throw new ParseException(
                    "--write 는 참여자 하나만 지목할 수 있다. 같은 워크스페이스에 여러 AI 를 "
                    + "동시에 쓰기 모드로 실행하지 않는다 (SPEC §7.5).");
        }
        String prompt = tailOf(raw, args, i);
        if (prompt.isEmpty()) {
            throw new ParseException("프롬프트가 비어 있다.\n" + USAGE);
        }
        return new RunCommand(targets, write, prompt);
    }

    /** raw 에서 앞쪽 토큰 i 개를 건너뛴 나머지를 원문 그대로 돌려준다. */
    private static String tailOf(String raw, List<String> args, int skip) {
        String rest = raw;
        for (int k = 0; k < skip && k < args.size(); k++) {
            int idx = rest.indexOf(args.get(k));
            if (idx < 0) break;
            rest = rest.substring(idx + args.get(k).length()).stripLeading();
        }
        return rest.strip();
    }
}
