package io.multiai.cli.process;

import java.nio.file.Path;
import java.util.List;

/**
 * 논리 이름(claude/codex/agy)에 대해 실제로 채택된 실행 경로.
 * SPEC §6.3 — 채택 경로와 강등 사유를 /status 에 표시하기 위해 보관한다.
 *
 * @param logicalName 논리 이름
 * @param launcher    실행 파일과 고정 선행 인수 (예: node.exe + codex.js)
 * @param executable  채택된 실행 파일 경로
 * @param tier        탐색 우선순위에서 몇 번째로 채택됐는지 (1 이 최우선)
 * @param note        강등 사유 등 사용자에게 보일 메모. 없으면 빈 문자열
 */
public record ResolvedCommand(
        String logicalName,
        List<String> launcher,
        Path executable,
        int tier,
        String note) {

    public boolean isPrimary() {
        return tier == 1;
    }
}
