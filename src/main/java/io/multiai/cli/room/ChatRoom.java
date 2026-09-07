package io.multiai.cli.room;

import java.io.IOException;
import java.nio.file.Path;
import java.util.*;

/**
 * 채팅방 하나. SPEC D6 — 이 기록이 공동 문맥의 SSOT 다.
 * 공급자 고유 세션 재개(codex resume / agy -c)는 쓰지 않는다.
 */
public final class ChatRoom {

    private final String id;
    private final String name;
    private final Path dir;
    private final Path workspace;
    private final List<ChatMessage> messages = new ArrayList<>();
    private int nextId = 1;
    private int round = 0;

    public ChatRoom(String id, String name, Path dir, Path workspace) {
        this.id = id;
        this.name = name;
        this.dir = dir;
        this.workspace = workspace;
    }

    public String id() { return id; }
    public String name() { return name; }
    public Path dir() { return dir; }
    public Path workspace() { return workspace; }
    public Path transcript() { return dir.resolve("transcript.md"); }
    public int nextId() { return nextId; }
    public int round() { return round; }
    public List<ChatMessage> messages() { return Collections.unmodifiableList(messages); }

    public int startRound() { return ++round; }

    /** 메시지를 추가하고 transcript 에 즉시 append 한다. */
    public ChatMessage add(String sender, String status, long ms, String body) throws IOException {
        ChatMessage m = new ChatMessage(nextId, round, sender, java.time.Instant.now(),
                status, ms, body, ChatMessage.State.OK);
        messages.add(m);
        nextId++;
        TranscriptCodec.append(transcript(), m);
        return m;
    }

    public ChatMessage addUser(String body) throws IOException {
        return add("user", "OK", 0, body);
    }

    /** 재개 시 호출. 복원된 메시지로 상태를 되살린다. */
    public void restore(List<ChatMessage> restored, int nextIdFromMeta, int roundFromMeta) {
        messages.clear();
        messages.addAll(restored);
        this.nextId = Math.max(nextIdFromMeta, restored.stream()
                .mapToInt(ChatMessage::id).max().orElse(0) + 1);
        this.round = roundFromMeta;
    }

    /** 복원 중 손상·의심으로 표시된 메시지 수. /open 시 사용자에게 알린다. */
    public long damagedCount() {
        return messages.stream().filter(m -> m.state() != ChatMessage.State.OK).count();
    }
}
