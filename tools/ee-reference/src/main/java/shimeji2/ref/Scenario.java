package shimeji2.ref;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/**
 * Shared scenario file. The grammar is documented in {@code tests/scenarios/schema.md}.
 */
public final class Scenario {
    public final String name;
    public final long seed;
    public final int ticks;
    public final String imageSet;
    public final List<ScreenSpec> screens;
    private final List<WindowKeyframe> windows;
    private final List<CursorKeyframe> cursors;
    private final List<MouseSpec> mouse;

    private Scenario(String name, long seed, int ticks, String imageSet, List<ScreenSpec> screens,
                     List<WindowKeyframe> windows, List<CursorKeyframe> cursors, List<MouseSpec> mouse) {
        this.name = name;
        this.seed = seed;
        this.ticks = ticks;
        this.imageSet = imageSet;
        this.screens = screens;
        this.windows = windows;
        this.cursors = cursors;
        this.mouse = mouse;
    }

    public static Scenario load(Path path) throws IOException {
        Object parsed = Json.parse(Files.readString(path, StandardCharsets.UTF_8));
        if (!(parsed instanceof Map<?, ?> root)) {
            throw new IllegalArgumentException(path + " is not a JSON object");
        }
        String name = string(root, "name");
        long seed = integer(root, "seed");
        int ticks = (int) integer(root, "ticks");
        if (ticks <= 0) {
            throw new IllegalArgumentException("ticks must be positive");
        }
        String imageSet = root.containsKey("imageSet") ? string(root, "imageSet") : "shime";
        List<ScreenSpec> screens = new ArrayList<>();
        for (Object item : array(root, "screens")) {
            Map<?, ?> screen = expectMap(item, "screen");
            int x = (int) integer(screen, "x");
            int y = (int) integer(screen, "y");
            int width = (int) integer(screen, "width");
            int height = (int) integer(screen, "height");
            int workX = x;
            int workY = y;
            int workWidth = width;
            int workHeight = height;
            if (screen.get("work") != null) {
                Map<?, ?> work = expectMap(screen.get("work"), "work");
                workX = (int) integer(work, "x");
                workY = (int) integer(work, "y");
                workWidth = (int) integer(work, "width");
                workHeight = (int) integer(work, "height");
            }
            screens.add(new ScreenSpec(string(screen, "id"), x, y, width, height, workX, workY, workWidth, workHeight));
        }
        if (screens.isEmpty()) {
            throw new IllegalArgumentException("screens must not be empty");
        }
        List<WindowKeyframe> windows = new ArrayList<>();
        if (root.get("activeWindow") instanceof List<?> list) {
            for (Object item : list) {
                Map<?, ?> frame = expectMap(item, "activeWindow");
                boolean visible = bool(frame, "visible", true);
                windows.add(new WindowKeyframe(
                        (int) integer(frame, "tick"),
                        visible,
                        visible ? (int) integer(frame, "x") : 0,
                        visible ? (int) integer(frame, "y") : 0,
                        visible ? (int) integer(frame, "width") : 0,
                        visible ? (int) integer(frame, "height") : 0,
                        frame.get("title") instanceof String title ? title : null));
            }
        }
        List<CursorKeyframe> cursors = new ArrayList<>();
        if (root.get("cursor") instanceof List<?> list) {
            for (Object item : list) {
                Map<?, ?> frame = expectMap(item, "cursor");
                cursors.add(new CursorKeyframe((int) integer(frame, "tick"), (int) integer(frame, "x"), (int) integer(frame, "y")));
            }
        }
        List<MouseSpec> mouse = new ArrayList<>();
        if (root.get("mouse") instanceof List<?> list) {
            for (Object item : list) {
                Map<?, ?> event = expectMap(item, "mouse");
                mouse.add(new MouseSpec(
                        (int) integer(event, "tick"),
                        string(event, "type"),
                        string(event, "button"),
                        event.containsKey("x") ? (int) integer(event, "x") : 0,
                        event.containsKey("y") ? (int) integer(event, "y") : 0,
                        event.get("mascot") instanceof String id ? id : null));
            }
        }
        windows.sort((a, b) -> Integer.compare(a.tick, b.tick));
        cursors.sort((a, b) -> Integer.compare(a.tick, b.tick));
        return new Scenario(name, seed, ticks, imageSet, List.copyOf(screens), List.copyOf(windows), List.copyOf(cursors), List.copyOf(mouse));
    }

    public WindowState windowAt(int tick) {
        if (windows.isEmpty()) {
            return WindowState.hidden();
        }
        return lerpWindow(tick);
    }

    public CursorState cursorAt(int tick) {
        if (cursors.isEmpty()) {
            return new CursorState(0, 0);
        }
        int hi = indexAtOrAfter(cursors, tick);
        if (hi < 0) {
            CursorKeyframe last = cursors.get(cursors.size() - 1);
            return new CursorState(last.x, last.y);
        }
        CursorKeyframe next = cursors.get(hi);
        if (hi == 0 || next.tick == tick) {
            return new CursorState(next.x, next.y);
        }
        CursorKeyframe prev = cursors.get(hi - 1);
        return new CursorState(lerp(prev.x, next.x, tick, prev.tick, next.tick), lerp(prev.y, next.y, tick, prev.tick, next.tick));
    }

    public List<MouseSpec> mouseAt(int tick) {
        List<MouseSpec> found = new ArrayList<>();
        for (MouseSpec event : mouse) {
            if (event.tick == tick) {
                found.add(event);
            }
        }
        return found;
    }

    private WindowState lerpWindow(int tick) {
        int hi = 0;
        while (hi < windows.size() && windows.get(hi).tick < tick) {
            hi++;
        }
        if (hi >= windows.size()) {
            return windows.get(windows.size() - 1).state();
        }
        WindowKeyframe next = windows.get(hi);
        if (hi == 0 || next.tick == tick) {
            return next.state();
        }
        WindowKeyframe prev = windows.get(hi - 1);
        if (prev.visible != next.visible) {
            return prev.state();
        }
        if (!prev.visible) {
            return WindowState.hidden();
        }
        return new WindowState(true,
                lerp(prev.x, next.x, tick, prev.tick, next.tick),
                lerp(prev.y, next.y, tick, prev.tick, next.tick),
                lerp(prev.width, next.width, tick, prev.tick, next.tick),
                lerp(prev.height, next.height, tick, prev.tick, next.tick),
                prev.title);
    }

    private static int indexAtOrAfter(List<? extends Timed> frames, int tick) {
        for (int i = 0; i < frames.size(); i++) {
            if (frames.get(i).tick() >= tick) {
                return i;
            }
        }
        return -1;
    }

    static int lerp(int start, int end, int tick, int tickStart, int tickEnd) {
        if (tickEnd == tickStart) {
            return end;
        }
        return start + (int) ((long) (end - start) * (tick - tickStart) / (tickEnd - tickStart));
    }

    public record ScreenSpec(String id, int x, int y, int width, int height, int workX, int workY, int workWidth, int workHeight) {
    }

    public record WindowState(boolean visible, int x, int y, int width, int height, String title) {
        static WindowState hidden() {
            return new WindowState(false, -500, -500, 0, 0, null);
        }
    }

    public record CursorState(int x, int y) {
    }

    public record MouseSpec(int tick, String type, String button, int x, int y, String mascot) {
    }

    private record WindowKeyframe(int tick, boolean visible, int x, int y, int width, int height, String title) implements Timed {
        WindowState state() {
            return new WindowState(visible, x, y, width, height, title);
        }
    }

    private record CursorKeyframe(int tick, int x, int y) implements Timed {
    }

    private interface Timed {
        int tick();
    }

    private static Map<?, ?> expectMap(Object value, String label) {
        if (value instanceof Map<?, ?> map) {
            return map;
        }
        throw new IllegalArgumentException(label + " must be an object");
    }

    private static List<?> array(Map<?, ?> object, String key) {
        Object value = object.get(key);
        if (value instanceof List<?> list) {
            return list;
        }
        throw new IllegalArgumentException(key + " must be an array");
    }

    private static String string(Map<?, ?> object, String key) {
        Object value = object.get(key);
        if (value instanceof String text) {
            return text;
        }
        throw new IllegalArgumentException(key + " must be a string");
    }

    private static long integer(Map<?, ?> object, String key) {
        Object value = object.get(key);
        if (value instanceof Long number) {
            return number;
        }
        throw new IllegalArgumentException(key + " must be an integer");
    }

    private static boolean bool(Map<?, ?> object, String key, boolean fallback) {
        Object value = object.get(key);
        if (value == null) {
            return fallback;
        }
        if (value instanceof Boolean flag) {
            return flag;
        }
        throw new IllegalArgumentException(key + " must be a boolean");
    }
}
