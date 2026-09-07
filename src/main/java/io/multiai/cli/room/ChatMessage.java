package io.multiai.cli.room;

import java.time.Instant;

/**
 * 채팅방의 메시지 한 건. SPEC §7.6 의 프레임 하나에 대응한다.
 *
 * @param id     방 안에서 1씩 증가. 여는·닫는 마커가 같은 값을 갖는다.
 * @param round  라운드 번호
 * @param sender "user" 또는 참여자 id
 * @param ts     기록 시각
 * @param status OK / FAILED / TIMEOUT / CANCELLED / UNPARSED
 * @param ms     소요 시간(ms). 사용자 메시지는 0
 * @param body   원문. 어떤 이스케이프도 하지 않는다
 * @param state  복원 상태. OK / SUSPECT / CORRUPT
 */
public record ChatMessage(
        int id, int round, String sender, Instant ts,
        String status, long ms, String body, State state) {

    public enum State { OK, SUSPECT, CORRUPT }

    public static ChatMessage user(int id, int round, String body) {
        return new ChatMessage(id, round, "user", Instant.now(), "OK", 0, body, State.OK);
    }

    public boolean isUser() {
        return "user".equals(sender);
    }
}
