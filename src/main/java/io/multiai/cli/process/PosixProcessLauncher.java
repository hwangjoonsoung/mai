package io.multiai.cli.process;

import java.nio.file.Path;
import java.util.List;

/**
 * macOS·POSIX 프로세스 기동·취소. SPEC §6.4.
 * 공통 로직은 {@link AbstractProcessLauncher} 에 있고 여기서는 강제 종료 폴백만 다르다.
 *
 * Windows 의 `taskkill /T` 에 정확히 대응하는 단일 명령이 POSIX 에는 없다.
 * 자손은 이미 공통 절차가 ProcessHandle 로 정리했으므로, 여기서는 남은 부모와
 * 그 직계를 두 단계로 친다.
 */
public final class PosixProcessLauncher extends AbstractProcessLauncher {

    public PosixProcessLauncher(Path fallbackDir) {
        super(fallbackDir);
    }

    @Override
    protected boolean forceKillTree(long pid) {
        // pkill -P: 부모가 살아 있는 동안 직계 자식을 먼저 정리한다.
        // 자식이 없어 실패해도 무방하므로 결과를 판정에 쓰지 않는다.
        runQuiet(List.of("pkill", "-9", "-P", Long.toString(pid)));
        return runQuiet(List.of("kill", "-9", Long.toString(pid)));
    }
}
