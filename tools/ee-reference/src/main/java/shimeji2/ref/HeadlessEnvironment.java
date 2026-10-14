package shimeji2.ref;

import com.group_finity.mascot.environment.Area;
import com.group_finity.mascot.environment.ComplexArea;
import com.group_finity.mascot.environment.Environment;
import com.group_finity.mascot.environment.Location;

import java.awt.Rectangle;
import java.util.Collection;
import java.util.HashMap;
import java.util.Map;

/**
 * Replays a scenario. Screen, work-area, cursor and active-window values come from the
 * scenario timeline. Cursor dx/dy are the smoothed values {@link Location#set(int, int)} computes.
 * Window and screen deltas are the raw deltas {@link Area#set} computes.
 */
public final class HeadlessEnvironment implements Environment {
    private final Scenario scenario;
    private final Area screen = new Area();
    private final ComplexArea complexScreen = new ComplexArea();
    private final ComplexArea complexWorkArea = new ComplexArea();
    private final Location cursor = new Location();
    private final Area activeWindow = new Area();
    private final Area invisible = new Area();
    private int nextTick;
    private boolean cursorPlaced;
    private String activeTitle;
    private long activeId;

    public HeadlessEnvironment(Scenario scenario) {
        this.scenario = scenario;
        invisible.setVisible(false);
        activeWindow.setVisible(false);
    }

    @Override
    public void init() {
        nextTick = 0;
        cursorPlaced = false;
        apply(0);
    }

    @Override
    public void tick() {
        apply(nextTick);
        nextTick++;
    }

    private void apply(int tick) {
        Map<String, Rectangle> screens = new HashMap<>();
        Map<String, Rectangle> workAreas = new HashMap<>();
        Rectangle union = null;
        for (Scenario.ScreenSpec spec : scenario.screens) {
            Rectangle bounds = new Rectangle(spec.x(), spec.y(), spec.width(), spec.height());
            screens.put(spec.id(), bounds);
            workAreas.put(spec.id(), new Rectangle(spec.workX(), spec.workY(), spec.workWidth(), spec.workHeight()));
            union = union == null ? new Rectangle(bounds) : union.union(bounds);
        }
        if (union == null) {
            throw new IllegalStateException("scenario has no screens");
        }
        screen.set(union);
        complexScreen.set(screens);
        complexWorkArea.set(workAreas);

        Scenario.WindowState window = scenario.windowAt(tick);
        activeTitle = window.title();
        if (!window.visible()) {
            activeWindow.setVisible(false);
            activeWindow.setRect(-500, -500, 0, 0);
            activeId = 0;
        } else {
            activeWindow.setVisible(true);
            activeWindow.setRect(window.x(), window.y(), window.width(), window.height());
            activeId = 1;
        }

        Scenario.CursorState point = scenario.cursorAt(tick);
        if (!cursorPlaced) {
            cursor.setX(point.x());
            cursor.setY(point.y());
            cursor.setDx(0);
            cursor.setDy(0);
            cursorPlaced = true;
        } else {
            cursor.set(point.x(), point.y());
        }
    }

    @Override
    public Area getWorkAreaAt(int x, int y) {
        Collection<Area> workAreas = complexWorkArea.getAreas();
        if (!workAreas.isEmpty()) {
            return workAreas.stream().filter(area -> area.contains(x, y)).findFirst().orElse(invisible);
        }
        return invisible;
    }

    @Override
    public ComplexArea getComplexWorkArea() {
        return complexWorkArea;
    }

    @Override
    public Area getScreen() {
        return screen;
    }

    @Override
    public Collection<Area> getScreens() {
        return complexScreen.getAreas();
    }

    @Override
    public ComplexArea getComplexScreen() {
        return complexScreen;
    }

    @Override
    public Area getActiveWindow() {
        return activeWindow;
    }

    @Override
    public String getActiveWindowTitle() {
        return activeTitle;
    }

    @Override
    public long getActiveWindowId() {
        return activeId;
    }

    @Override
    public void moveActiveWindow(int x, int y) {
        if (!activeWindow.isVisible()) {
            return;
        }
        activeWindow.setRect(x, y, activeWindow.getWidth(), activeWindow.getHeight());
    }

    @Override
    public void restoreWindows() {
    }

    @Override
    public Location getCursor() {
        return cursor;
    }

    @Override
    public void refreshCache() {
    }

    @Override
    public void dispose() {
    }
}
