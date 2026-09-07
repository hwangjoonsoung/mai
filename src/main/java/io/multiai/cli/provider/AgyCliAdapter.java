package io.multiai.cli.provider;

import io.multiai.cli.process.*;

import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;

/**
 * Antigravity CLI(agy) 어댑터 — 화면 표시는 Gemini. SPEC §7.2.
 *
 * 읽기 전용: -p <prompt> --add-dir <ws> --model <id> --mode plan --sandbox
 *            --disable-slash-commands --output-format text --print-timeout 11m
 * 쓰기 허용: --mode accept-edits (--sandbox 유지)
 *
 * agy 는 stdin 으로 프롬프트를 받지 못한다 — -p 가 값을 요구하는 플래그이고
 * -p=- 는 "-" 를 리터럴 프롬프트로 처리한다(§5.3 실측). 따라서 인수로 전달하며,
 * 이것이 전 공급자 공통 상한을 16,000자로 맞춘 이유다(D15).
 *
 * --print-timeout 은 Java 타임아웃(600초)보다 길게 둔다(D16). CLI 자체 타임아웃은
 * Java 감시가 실패했을 때만 작동하는 보조 안전장치다.
 */
public final class AgyCliAdapter extends AbstractCliProvider {

    private final String model;

    public AgyCliAdapter(ResolvedCommand c, ProcessLauncher l, String model) {
        super(c, l);
        this.model = model;
    }

    @Override public String id() { return "gemini"; }
    @Override public String displayName() { return "Gemini via agy"; }
    @Override public boolean acceptsStdin() { return false; }

    @Override
    protected List<String> arguments(String prompt, Path ws, boolean write,
                                     Path tempDir, Duration timeout, Path schemaFile) {
        long cliSeconds = timeout.toSeconds() + 60;   // Java 보다 길게
        List<String> a = new ArrayList<>(List.of(
                "-p", prompt,
                "--add-dir", ws.toString(),
                "--model", model,
                "--mode", write ? "accept-edits" : "plan",
                "--sandbox",
                "--disable-slash-commands",
                "--output-format", schemaFile == null ? "text" : "json",
                "--print-timeout", cliSeconds + "s"));
        if (schemaFile != null) {
            // 반드시 파일 경로로 전달한다. 인라인 JSON 은 Windows 에서 깨진다 (§1.5).
            a.add("--json-schema");
            a.add(schemaFile.toString());
        }
        return a;
    }

    @Override
    protected String extractText(RunOutcome out, Path tempDir) {
        return out.stdout().strip();
    }
}
