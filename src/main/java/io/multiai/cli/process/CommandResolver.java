package io.multiai.cli.process;

import java.util.Optional;

/**
 * 논리 이름을 실제 실행 경로로 해석한다.
 * SPEC §6.4 — OS 판별은 이 인터페이스 구현체 안에서만 한다.
 */
public interface CommandResolver {

    /** 해석에 성공하면 채택된 명령을, 실패하면 empty 를 돌려준다. */
    Optional<ResolvedCommand> resolve(String logicalName);
}
