package io.multiai.cli.process;

import java.nio.file.Path;
import java.util.List;

/**
 * 한 번의 CLI 실행 요청. SPEC §6.3 — 인수는 문자열 배열로만 전달하고
 * 셸 문자열로 연결하지 않는다.
 *
 * @param command    실행 파일과 인수. command.get(0) 이 실행 파일이다.
 * @param workspace  작업 디렉터리 (SPEC D18). 절대 경로로 정규화된 값.
 * @param stdinBody  stdin 으로 주입할 프롬프트. null 이면 주입하지 않고 즉시 close.
 * @param timeout    Java 가 강제하는 타임아웃 (SPEC D16 — 공통 600초)
 * @param label      로그·오류 표시에 쓰는 참여자 이름
 * @param runDir     이 라운드의 원본 stdout·stderr 를 남길 디렉터리
 */
public record Invocation(
        List<String> command,
        Path workspace,
        String stdinBody,
        java.time.Duration timeout,
        String label,
        Path runDir) {

    public Invocation {
        if (command == null || command.isEmpty()) {
            throw new IllegalArgumentException("command 가 비어 있다");
        }
    }
}
