package io.multiai.cli.room;

import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.time.format.DateTimeFormatter;
import java.time.ZoneId;
import java.time.Instant;
import java.util.*;

/**
 * 방 저장소. SPEC §7.6 「저장 규약」.
 *
 * %USERPROFILE%\.multi-ai-cli\
 *   config.properties
 *   temp\                      공급자 임시 출력 (워크스페이스 밖)
 *   rooms\<room-id>\
 *     room.properties
 *     transcript.md
 *     runs\<round-id>\*.stdout.txt, *.stderr.txt
 *
 * 설정은 .properties, 대화는 Markdown 으로 저장한다 — 외부 라이브러리 없이(D12).
 */
public final class RoomRepository {

    private static final DateTimeFormatter STAMP =
            DateTimeFormatter.ofPattern("yyyyMMdd-HHmmss").withZone(ZoneId.systemDefault());

    private final Path home;

    public RoomRepository(Path home) {
        this.home = home;
    }

    /** SPEC §7.6 — macOS 에서도 논리 구조는 ~/.multi-ai-cli/ 로 동일하다. */
    public static Path defaultHome() {
        return io.multiai.cli.process.Platform.home();
    }

    public Path home() { return home; }
    public Path roomsDir() { return home.resolve("rooms"); }
    public Path tempDir() { return home.resolve("temp"); }
    public Path configFile() { return home.resolve("config.properties"); }

    public void ensureLayout() throws IOException {
        Files.createDirectories(roomsDir());
        Files.createDirectories(tempDir());
    }

    // ---------- 설정 ----------

    public Properties loadConfig() throws IOException {
        Properties p = new Properties();
        if (Files.exists(configFile())) {
            try (Reader r = Files.newBufferedReader(configFile(), StandardCharsets.UTF_8)) {
                p.load(r);
            }
        }
        return p;
    }

    // ---------- 방 ----------

    public ChatRoom create(String name, Path workspace) throws IOException {
        ensureLayout();
        String id = STAMP.format(Instant.now());
        Path dir = roomsDir().resolve(id);
        Files.createDirectories(dir.resolve("runs"));
        ChatRoom room = new ChatRoom(id, name == null || name.isBlank() ? id : name, dir, workspace);
        saveMeta(room);
        return room;
    }

    public List<RoomSummary> list() throws IOException {
        if (!Files.isDirectory(roomsDir())) return List.of();
        List<RoomSummary> out = new ArrayList<>();
        try (DirectoryStream<Path> ds = Files.newDirectoryStream(roomsDir(), Files::isDirectory)) {
            for (Path d : ds) {
                Properties p = readProps(d.resolve("room.properties"));
                out.add(new RoomSummary(
                        d.getFileName().toString(),
                        p.getProperty("name", d.getFileName().toString()),
                        p.getProperty("workspace", ""),
                        Integer.parseInt(p.getProperty("round", "0"))));
            }
        }
        out.sort(Comparator.comparing(RoomSummary::id).reversed());
        return out;
    }

    /**
     * 방을 연다. SPEC §7.8 D18-5 — 저장된 워크스페이스가 없어졌거나 현재와 다르면
     * 조용히 대체하지 않고 호출자에게 알린다.
     */
    public ChatRoom open(String id) throws IOException {
        Path dir = roomsDir().resolve(id);
        if (!Files.isDirectory(dir)) throw new NoSuchFileException("방 없음: " + id);
        Properties p = readProps(dir.resolve("room.properties"));
        int nextId = Integer.parseInt(p.getProperty("next_id", "1"));
        int round = Integer.parseInt(p.getProperty("round", "0"));
        Path ws = Path.of(p.getProperty("workspace", System.getProperty("user.dir")));
        ChatRoom room = new ChatRoom(id, p.getProperty("name", id), dir, ws);
        room.restore(TranscriptCodec.readAll(dir.resolve("transcript.md"), nextId), nextId, round);
        return room;
    }

    public void saveMeta(ChatRoom room) throws IOException {
        Properties p = new Properties();
        p.setProperty("id", room.id());
        p.setProperty("name", room.name());
        p.setProperty("workspace", room.workspace().toString());
        p.setProperty("next_id", Integer.toString(room.nextId()));
        p.setProperty("round", Integer.toString(room.round()));
        try (Writer w = Files.newBufferedWriter(room.dir().resolve("room.properties"),
                StandardCharsets.UTF_8)) {
            p.store(w, "multi_ai_cli room metadata");
        }
    }

    public Path runDir(ChatRoom room, int round) throws IOException {
        Path d = room.dir().resolve("runs").resolve(String.format("r%04d", round));
        Files.createDirectories(d);
        return d;
    }

    private static Properties readProps(Path f) throws IOException {
        Properties p = new Properties();
        if (Files.exists(f)) {
            try (Reader r = Files.newBufferedReader(f, StandardCharsets.UTF_8)) {
                p.load(r);
            }
        }
        return p;
    }

    public record RoomSummary(String id, String name, String workspace, int round) {}
}
