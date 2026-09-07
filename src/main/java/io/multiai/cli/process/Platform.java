package io.multiai.cli.process;

import java.nio.file.Path;
import java.util.Locale;
import java.util.Map;

/**
 * OS 판별의 **유일한** 지점. SPEC §6.4.
 *
 * "다음 인터페이스 밖에서는 OS 를 판별하지 않는다" 는 규칙을 지키기 위해,
 * 앱 전체에서 os.name 을 보는 곳을 여기 하나로 모은다. 공통 비즈니스 로직은
 * CommandResolver·ProcessLauncher 계약만 알면 되고 OS 를 알 필요가 없다.
 */
public final class Platform {

    public enum Os { WINDOWS, MAC, OTHER_POSIX }

    private Platform() {}

    public static Os current() {
        String n = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        if (n.contains("win")) return Os.WINDOWS;
        if (n.contains("mac") || n.contains("darwin")) return Os.MAC;
        return Os.OTHER_POSIX;
    }

    public static CommandResolver resolver(Map<String, Path> overrides) {
        return current() == Os.WINDOWS
                ? new WindowsCommandResolver(overrides)
                : new MacCommandResolver(overrides);
    }

    public static ProcessLauncher launcher(Path fallbackDir) {
        return current() == Os.WINDOWS
                ? new WindowsProcessLauncher(fallbackDir)
                : new PosixProcessLauncher(fallbackDir);
    }

    /**
     * 저장 위치. SPEC §7.6 — macOS 에서도 논리 구조는 ~/.multi-ai-cli/ 로 동일하다.
     */
    public static Path home() {
        if (current() == Os.WINDOWS) {
            String up = System.getenv("USERPROFILE");
            if (up != null && !up.isBlank()) return Path.of(up).resolve(".multi-ai-cli");
        }
        return Path.of(System.getProperty("user.home")).resolve(".multi-ai-cli");
    }
}
