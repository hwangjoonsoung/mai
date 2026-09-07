package io.multiai.cli.provider;

import io.multiai.cli.process.*;

import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;

/**
 * Claude Code CLI 어댑터. SPEC §7.2.
 *
 * 읽기 전용:  -p --add-dir <ws> --input-format text --output-format text
 *             --restricted --permission-mode plan --permission-prompts none --tools ""
 * 쓰기 허용:  --permission-mode acceptEdits (그 외 동일)
 *
 * --restricted 는 필수다(D17). 없으면 Claude 만 사용자 CLAUDE.md·MCP 설정을
 * 상속해 세 공급자의 출발 조건이 달라진다.
 * 프롬프트는 stdin 으로 주입한다(§5.3 실측).
 */
public final class ClaudeCliAdapter extends AbstractCliProvider {

    public ClaudeCliAdapter(ResolvedCommand c, ProcessLauncher l) { super(c, l); }

    @Override public String id() { return "claude"; }
    @Override public String displayName() { return "Claude"; }
    @Override public boolean acceptsStdin() { return true; }

    @Override
    protected List<String> arguments(String prompt, Path ws, boolean write,
                                     Path tempDir, Duration timeout, Path schemaFile) {
        List<String> a = new ArrayList<>(List.of(
                "-p",
                "--add-dir", ws.toString(),
                "--input-format", "text",
                "--output-format", "text",
                "--restricted",
                "--permission-mode", write ? "acceptEdits" : "plan",
                "--permission-prompts", "none",
                "--tools", write ? "default" : ""));
        // --json-schema 는 쓰지 않는다. 두 가지가 겹쳐 Windows 에서 성립하지 않는다:
        //   1. claude 는 파일 경로가 아니라 스키마 **문자열**만 받는다 (경로를 주면
        //      "Unexpected identifier" 로 거부).
        //   2. 그 문자열에는 따옴표가 가득한데, Windows ProcessBuilder 는 따옴표가 든
        //      인자를 온전히 전달하지 못해 "not valid JSON" 이 된다 (SPEC §1.5 경고).
        // 대신 프롬프트에 스키마를 실어 보내고 text 출력에서 JSON 을 추출한다.
        return a;
    }

    @Override
    protected String extractText(RunOutcome out, Path tempDir) {
        return out.stdout().strip();
    }
}
