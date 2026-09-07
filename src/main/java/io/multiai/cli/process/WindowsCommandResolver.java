package io.multiai.cli.process;

import java.io.IOException;
import java.nio.file.*;
import java.util.*;
import java.util.stream.Stream;

/**
 * Windows 탐색기. SPEC §6.3 「공급자별 실행 파일 탐색 우선순위」.
 *
 * claude : claude.exe 직접 실행
 * codex  : ① 벤더 네이티브 codex.exe → ② node.exe + codex.js → ③ 지원 불가
 * agy    : agy.exe 직접 실행
 *
 * codex.cmd / codex.ps1 폴백은 사용하지 않는다 — 셸 재해석으로 프롬프트가
 * 손상되고 §7.8 의 보안 경계가 무너진다 (교차검토 R2).
 */
public final class WindowsCommandResolver implements CommandResolver {

    private final Map<String, Path> overrides;

    public WindowsCommandResolver(Map<String, Path> overrides) {
        this.overrides = overrides == null ? Map.of() : overrides;
    }

    @Override
    public Optional<ResolvedCommand> resolve(String logicalName) {
        Path override = overrides.get(logicalName);
        if (override != null && Files.isExecutable(override)) {
            return Optional.of(new ResolvedCommand(
                    logicalName, List.of(override.toString()), override, 1, "config.properties 지정"));
        }
        return switch (logicalName) {
            case "claude", "agy" -> resolveDirectExe(logicalName);
            case "codex" -> resolveCodex();
            default -> Optional.empty();
        };
    }

    /** PATH 에서 <name>.exe 를 찾아 직접 실행한다. */
    private Optional<ResolvedCommand> resolveDirectExe(String name) {
        return which(name + ".exe")
                .map(p -> new ResolvedCommand(name, List.of(p.toString()), p, 1, ""));
    }

    private Optional<ResolvedCommand> resolveCodex() {
        // ① 벤더 네이티브 codex.exe — npm shim 이 감싸고 있는 실제 바이너리
        Optional<Path> native_ = findVendorCodexExe();
        if (native_.isPresent()) {
            Path p = native_.get();
            return Optional.of(new ResolvedCommand("codex", List.of(p.toString()), p, 1, ""));
        }
        // ② node.exe + codex.js — shim 이 하는 일을 셸 없이 그대로 재현
        Optional<Path> js = findCodexJs();
        Optional<Path> node = which("node.exe");
        if (js.isPresent() && node.isPresent()) {
            return Optional.of(new ResolvedCommand("codex",
                    List.of(node.get().toString(), js.get().toString()),
                    js.get(), 2,
                    "네이티브 codex.exe 미발견 — node + codex.js 로 강등"));
        }
        // ③ 지원 불가. codex.cmd / codex.ps1 로 내려가지 않는다.
        return Optional.empty();
    }

    /** npm 전역 설치 경로 아래에서 벤더 바이너리를 글롭 탐색한다. */
    private Optional<Path> findVendorCodexExe() {
        for (Path root : npmRoots()) {
            Path base = root.resolve("node_modules/@openai/codex/node_modules");
            if (!Files.isDirectory(base)) continue;
            // 실측 경로는 base 기준 6단계다:
            //   @openai/codex-win32-x64/vendor/<triple>/bin/codex.exe
            // 패키지 레이아웃이 바뀔 수 있으니 여유를 둔다.
            try (Stream<Path> s = Files.walk(base, 8)) {
                Optional<Path> hit = s
                        .filter(p -> p.getFileName().toString().equals("codex.exe"))
                        .filter(p -> p.toString().replace('\\', '/').contains("/vendor/"))
                        .filter(Files::isExecutable)
                        .findFirst();
                if (hit.isPresent()) return hit;
            } catch (IOException ignored) {
                // 탐색 실패는 조용히 넘기고 다음 후보로. 최종 실패는 ③ 에서 보고한다.
            }
        }
        return Optional.empty();
    }

    private Optional<Path> findCodexJs() {
        for (Path root : npmRoots()) {
            Path js = root.resolve("node_modules/@openai/codex/bin/codex.js");
            if (Files.isRegularFile(js)) return Optional.of(js);
        }
        return Optional.empty();
    }

    private List<Path> npmRoots() {
        List<Path> out = new ArrayList<>();
        String appData = System.getenv("APPDATA");
        if (appData != null) out.add(Path.of(appData, "npm"));
        which("codex.cmd").map(Path::getParent).ifPresent(out::add);
        return out;
    }

    /** PowerShell 없이 PATH 와 PATHEXT 만으로 실행 파일을 찾는다. */
    private Optional<Path> which(String fileName) {
        String path = System.getenv("PATH");
        if (path == null) return Optional.empty();
        String sep = java.util.regex.Pattern.quote(java.io.File.pathSeparator);
        for (String dir : path.split(sep)) {
            if (dir.isBlank()) continue;
            try {
                Path c = Path.of(dir.trim(), fileName);
                if (Files.isRegularFile(c)) return Optional.of(c.toAbsolutePath().normalize());
            } catch (InvalidPathException ignored) {
                // PATH 에 잘못된 항목이 섞여 있어도 탐색을 계속한다.
            }
        }
        return Optional.empty();
    }
}
