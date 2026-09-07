package io.multiai.cli.provider;

import io.multiai.cli.process.*;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;

/**
 * Codex CLI 어댑터. SPEC §7.2.
 *
 * 읽기 전용: codex exec - --skip-git-repo-check -C <ws> -s read-only
 *            -c model_reasoning_effort="high" -o <temp>/<run-id>.md
 * 쓰기 허용: -s workspace-write (그 외 동일)
 *
 * 끝의 "-" 가 stdin 에서 프롬프트를 읽으라는 지시다 (codex exec --help 실측).
 * -o 임시 파일은 워크스페이스가 아니라 %USERPROFILE%\.multi-ai-cli\temp\ 에 만든다 —
 * read-only 샌드박스와의 충돌을 피하고 워크스페이스를 오염시키지 않는다(§7.6).
 * 파일은 try-finally 로 정리하고, 없거나 비면 stdout 으로 폴백한다.
 */
public final class CodexCliAdapter extends AbstractCliProvider {

    private volatile Path lastOutFile;
    private final String modelOverride;

    public CodexCliAdapter(ResolvedCommand c, ProcessLauncher l) {
        this(c, l, null);
    }

    /** @param modelOverride config.properties 의 codex.model. null 이면 CLI 기본값 */
    public CodexCliAdapter(ResolvedCommand c, ProcessLauncher l, String modelOverride) {
        super(c, l);
        this.modelOverride = modelOverride;
    }

    @Override public String id() { return "codex"; }
    @Override public String displayName() { return "Codex"; }
    @Override public boolean acceptsStdin() { return true; }

    @Override
    protected List<String> arguments(String prompt, Path ws, boolean write,
                                     Path tempDir, Duration timeout, Path schemaFile) {
        Path out = tempDir.resolve(UUID.randomUUID() + ".md");
        lastOutFile = out;
        List<String> a = new ArrayList<>(List.of(
                "exec", "-",
                "--skip-git-repo-check",
                "-C", ws.toString(),
                "-s", write ? "workspace-write" : "read-only",
                "-c", "model_reasoning_effort=\"high\"",
                "-o", out.toString()));
        if (modelOverride != null && !modelOverride.isBlank()) {
            // 계정에 따라 기본 모델이 요청을 거부할 수 있다 — 실측 사례:
            // "The 'gpt-5.6-sol' model is not supported when using Codex with a ChatGPT account."
            a.add("-c");
            a.add("model=\"" + modelOverride.trim() + "\"");
        }
        if (schemaFile != null) {
            // CLI 레벨 스키마 강제. 스키마의 모든 object 에 additionalProperties:false 가
            // 있어야 한다 — OpenAI strict 구조화 출력 요구사항이다 (실측).
            //
            // 계정 권한에 따라 이 요청이 400 으로 거부될 수 있다("model is not supported
            // when using Codex with a ChatGPT account"). 그때도 프롬프트에 스키마가
            // 실려 있으므로 형식은 유지되고, 실패 시 라운드는 PARTIAL 로 진행한다.
            a.add("--output-schema");
            a.add(schemaFile.toString());
        }
        return a;
    }

    @Override
    protected String extractText(RunOutcome out, Path tempDir) {
        Path f = lastOutFile;
        try {
            if (f != null && Files.isRegularFile(f)) {
                String s = Files.readString(f, StandardCharsets.UTF_8).strip();
                if (!s.isBlank()) return s;
            }
        } catch (IOException ignored) {
            // 파일이 불완전하면 스트림으로 폴백한다.
        } finally {
            deleteQuietly(f);
        }
        return out.stdout().strip();
    }

    private static void deleteQuietly(Path f) {
        if (f == null) return;
        try {
            Files.deleteIfExists(f);
        } catch (IOException ignored) {
            // 정리 실패가 라운드를 중단시키지 않는다.
        }
    }
}
