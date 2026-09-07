package io.multiai.cli.provider;

import java.nio.file.Path;
import java.time.Duration;

/**
 * 정규화된 공급자 실행 결과. SPEC §7.3.
 * 공급자별 출력 형식을 UI·오케스트레이션 계층이 직접 해석하지 않게 한다.
 */
public record ProviderResult(
        String providerId,
        String displayName,
        Status status,
        String text,
        int exitCode,
        Duration elapsed,
        Path stdoutFile,
        Path stderrFile,
        String failureNote) {

    public enum Status { OK, FAILED, TIMEOUT, CANCELLED, UNPARSED, UNAVAILABLE }

    public boolean ok() {
        return status == Status.OK || status == Status.UNPARSED;
    }
}
