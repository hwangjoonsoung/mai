package io.multiai.cli.process;

import java.nio.file.Path;
import java.util.List;

/**
 * Windows 프로세스 기동·취소. SPEC §6.3.
 * 공통 로직은 {@link AbstractProcessLauncher} 에 있고 여기서는 강제 종료 폴백만 다르다.
 */
public final class WindowsProcessLauncher extends AbstractProcessLauncher {

    public WindowsProcessLauncher(Path fallbackDir) {
        super(fallbackDir);
    }

    /**
     * Windows 내장 유틸리티. 부모가 살아 있는 동안 호출해야 /T 가 남은 트리를
     * 함께 정리할 수 있다 (§6.3 5단계).
     */
    @Override
    protected boolean forceKillTree(long pid) {
        return runQuiet(List.of("taskkill", "/PID", Long.toString(pid), "/T", "/F"));
    }
}
