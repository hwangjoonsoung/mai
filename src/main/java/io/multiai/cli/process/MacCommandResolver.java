package io.multiai.cli.process;

import java.io.IOException;
import java.nio.file.*;
import java.util.*;
import java.util.stream.Stream;

/**
 * macOS·POSIX 탐색기. SPEC §6.4 「macOS 이식 경계」.
 *
 * Windows 판과 같은 계약을 따르되 아래만 다르다.
 *   - 확장자가 없다. PATH 의 실행 비트로 판정한다 (`command -v` 에 해당).
 *   - Homebrew·nvm 등 PATH 에 안 잡히는 흔한 위치를 보조로 살핀다.
 *   - agy 는 ~/.local/bin/agy 를 우선 확인한다 (SPEC §8.4 K6).
 *
 * codex 는 Windows 와 동일한 이유로 셸 래퍼(.cmd/.ps1 에 해당하는 sh shim)를
 * 경유하지 않는다 — 네이티브 바이너리 또는 node + codex.js 직접 실행만 쓴다.
 */
public final class MacCommandResolver implements CommandResolver {

    private final Map<String, Path> overrides;

    public MacCommandResolver(Map<String, Path> overrides) {
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
            case "claude" -> direct("claude");
            case "agy" -> agy();
            case "codex" -> codex();
            default -> Optional.empty();
        };
    }

    private Optional<ResolvedCommand> direct(String name) {
        return which(name).map(p -> new ResolvedCommand(name, List.of(p.toString()), p, 1, ""));
    }

    /** SPEC §8.4 K6 — macOS 는 ~/.local/bin/agy 와 PATH 를 우선 탐색한다. */
    private Optional<ResolvedCommand> agy() {
        Path local = home().resolve(".local/bin/agy");
        if (Files.isExecutable(local)) {
            return Optional.of(new ResolvedCommand("agy", List.of(local.toString()), local, 1, ""));
        }
        return direct("agy");
    }

    private Optional<ResolvedCommand> codex() {
        Optional<Path> nativeExe = findVendorCodex();
        if (nativeExe.isPresent()) {
            Path p = nativeExe.get();
            return Optional.of(new ResolvedCommand("codex", List.of(p.toString()), p, 1, ""));
        }
        Optional<Path> js = findCodexJs();
        Optional<Path> node = which("node");
        if (js.isPresent() && node.isPresent()) {
            return Optional.of(new ResolvedCommand("codex",
                    List.of(node.get().toString(), js.get().toString()), js.get(), 2,
                    "네이티브 codex 미발견 — node + codex.js 로 강등"));
        }
        return Optional.empty();
    }

    private Optional<Path> findVendorCodex() {
        for (Path root : npmRoots()) {
            Path base = root.resolve("node_modules/@openai/codex/node_modules");
            if (!Files.isDirectory(base)) continue;
            try (Stream<Path> s = Files.walk(base, 8)) {
                Optional<Path> hit = s
                        .filter(p -> p.getFileName().toString().equals("codex"))
                        .filter(p -> p.toString().contains("/vendor/"))
                        .filter(Files::isExecutable)
                        .filter(Files::isRegularFile)
                        .findFirst();
                if (hit.isPresent()) return hit;
            } catch (IOException ignored) {
                // 탐색 실패는 다음 후보로 넘긴다.
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

    /** npm prefix 후보. Homebrew(Intel/Apple Silicon)와 사용자 설치를 함께 본다. */
    private List<Path> npmRoots() {
        List<Path> out = new ArrayList<>();
        out.add(Path.of("/opt/homebrew/lib"));
        out.add(Path.of("/usr/local/lib"));
        out.add(home().resolve(".npm-global/lib"));
        which("codex").map(Path::getParent).map(Path::getParent).map(p -> p.resolve("lib"))
                .ifPresent(out::add);
        return out;
    }

    /** POSIX `command -v` 에 해당. 실행 비트로 판정한다. */
    private Optional<Path> which(String name) {
        String path = System.getenv("PATH");
        if (path == null) return Optional.empty();
        List<String> dirs = new ArrayList<>(Arrays.asList(path.split(":")));
        // GUI 앱에서 실행하면 PATH 가 빈약하다. 흔한 위치를 보조로 덧붙인다.
        dirs.addAll(List.of("/opt/homebrew/bin", "/usr/local/bin",
                home().resolve(".local/bin").toString()));
        for (String dir : dirs) {
            if (dir.isBlank()) continue;
            try {
                Path c = Path.of(dir.trim(), name);
                if (Files.isRegularFile(c) && Files.isExecutable(c)) {
                    return Optional.of(c.toAbsolutePath().normalize());
                }
            } catch (InvalidPathException ignored) {
                // PATH 에 잘못된 항목이 있어도 계속한다.
            }
        }
        return Optional.empty();
    }

    private static Path home() {
        return Path.of(System.getProperty("user.home"));
    }
}
