package io.multiai.cli.process;

import java.nio.file.Path;
import java.time.Duration;

/**
 * 프로세스 실행의 원시 결과. 공급자별 해석 전 단계다.
 * SPEC §7.3 — 원본 stdout·stderr 를 정규화된 메시지와 분리 보관한다.
 */
public record RunOutcome(
        Status status,
        int exitCode,
        String stdout,
        String stderr,
        Path stdoutFile,
        Path stderrFile,
        Duration elapsed,
        String failureNote) {

    public enum Status { OK, FAILED, TIMEOUT, CANCELLED, LAUNCH_ERROR }

    public boolean ok() {
        return status == Status.OK;
    }
}
