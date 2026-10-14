package shimeji2.ref;

import com.group_finity.mascot.Main;
import com.group_finity.mascot.Manager;
import com.group_finity.mascot.Mascot;
import com.group_finity.mascot.Settings;
import com.group_finity.mascot.action.Action;
import com.group_finity.mascot.action.ActionBase;
import com.group_finity.mascot.action.ComplexAction;
import com.group_finity.mascot.behavior.Behavior;
import com.group_finity.mascot.behavior.UserBehavior;
import com.group_finity.mascot.script.Constant;
import com.group_finity.mascot.script.Script;
import com.group_finity.mascot.script.Variable;
import com.group_finity.mascot.script.VariableMap;
import org.openjdk.nashorn.api.scripting.NashornScriptEngine;

import javax.script.ScriptContext;
import javax.swing.JPanel;
import javax.swing.SwingUtilities;
import java.awt.event.MouseEvent;
import java.io.OutputStreamWriter;
import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Locale;
import java.util.Random;
import java.util.ResourceBundle;

/**
 * Steps the vendored Shimeji-Desktop engine one tick at a time and writes a trace.
 * One process runs one scenario. Swing's event thread is non-daemon, so the process
 * exits with {@link System#exit(int)} after the trace is flushed.
 */
public final class TraceRunner {
    private static final long PROBE_SEED = 123456789L;
    private static final JPanel EVENT_SOURCE = new JPanel();

    private TraceRunner() {
    }

    public static void main(String[] args) {
        try {
            run(args);
            System.exit(0);
        } catch (Throwable failure) {
            failure.printStackTrace(System.err);
            System.exit(1);
        }
    }

    private static void run(String[] args) throws Exception {
        Args parsed = Args.parse(args);
        System.setProperty("shimeji2.ref.factory", "headless");
        Locale.setDefault(Locale.JAPAN);

        if (!Files.isRegularFile(Path.of("conf", "logging.properties"))) {
            throw new IllegalStateException("working directory must be the prepared run dir (conf/logging.properties is missing)");
        }

        Scenario scenario = Scenario.load(parsed.scenario);
        int ticks = parsed.ticks > 0 ? parsed.ticks : scenario.ticks;

        HeadlessEnvironment environment = new HeadlessEnvironment(scenario);
        HeadlessHolder.environment = environment;

        RngScan.scanEngineClasses();
        NashornScriptEngine engine = nashorn();
        installRandom(engine);
        probeRandom(engine);
        SeededRandom.reseed(scenario.seed);

        Main main = Main.getInstance();
        invoke(main, "loadLanguage", new Class<?>[]{Locale.class}, Locale.JAPAN);
        Object loaded = invoke(main, "loadConfiguration", new Class<?>[]{String.class}, scenario.imageSet);
        if (!Boolean.TRUE.equals(loaded)) {
            throw new IllegalStateException("failed to load configuration for image set " + scenario.imageSet);
        }

        Settings settings = main.getSettings();
        settings.sounds = false;
        settings.breeding = true;
        settings.multiscreen = true;
        settings.scaling = 1.0;
        settings.opacity = 1.0;
        settings.drawShimejiBounds = false;
        settings.throwing = true;
        settings.transformation = true;
        settings.transients = true;
        settings.language = Locale.JAPAN;

        Manager manager = (Manager) invoke(main, "getManager", new Class<?>[]{}, new Object[]{});
        manager.setExitOnLastRemoved(false);

        environment.init();
        main.createMascot(scenario.imageSet);
        SwingUtilities.invokeAndWait(() -> {
        });

        Method tick = Manager.class.getDeclaredMethod("tick");
        tick.setAccessible(true);
        Field mascotsField = Manager.class.getDeclaredField("mascots");
        mascotsField.setAccessible(true);

        Path outPath = parsed.out;
        if (outPath.getParent() != null) {
            Files.createDirectories(outPath.getParent());
        }
        try (PrintWriter out = new PrintWriter(new OutputStreamWriter(Files.newOutputStream(outPath), StandardCharsets.UTF_8))) {
            EvalTrace.out = out;
            EvalTrace.enabled = parsed.verbose;
            writeHeader(out, scenario, ticks, engine);
            for (int i = 0; i < ticks; i++) {
                EvalTrace.tick = i;
                injectMouse(manager, mascotsField, scenario.mouseAt(i));
                try {
                    tick.invoke(manager);
                } catch (InvocationTargetException e) {
                    Throwable cause = unwrap(e);
                    writeAbort(out, i, cause);
                    System.err.println("abort tick=" + i + " " + cause.getClass().getName()
                            + " " + (cause.getMessage() == null ? "-" : cause.getMessage()));
                    break;
                }
                writeTick(out, i, mascots(manager, mascotsField));
            }
            out.flush();
        } finally {
            EvalTrace.out = null;
            EvalTrace.enabled = false;
        }
    }

    private static void writeHeader(PrintWriter out, Scenario scenario, int ticks, NashornScriptEngine engine) {
        String pin = System.getProperty("shimeji2.ref.pin", "unknown");
        String nashornVersion = engine.getClass().getPackage().getImplementationVersion();
        if (nashornVersion == null || nashornVersion.isBlank()) {
            nashornVersion = "15.7";
        }
        out.print("# scenario\t");
        out.print(TraceFormat.escape(scenario.name));
        out.print('\n');
        out.print("# seed\t");
        out.print(scenario.seed);
        out.print('\n');
        out.print("# ticks\t");
        out.print(ticks);
        out.print('\n');
        out.print("# engine\tDalekCraft2/Shimeji-Desktop\n");
        out.print("# commit\t");
        out.print(pin);
        out.print('\n');
        out.print("# script-engine\t");
        out.print(engine.getClass().getName());
        out.print('\n');
        out.print("# nashorn\t");
        out.print(nashornVersion);
        out.print('\n');
        out.print("# jdk\t");
        out.print(System.getProperty("java.version"));
        out.print('\n');
        out.print("# rng-counter\tshared-after-tick\n");
    }

    /**
     * An exception that escapes {@code Manager.tick} is the reference's behavior for that tick.
     * The line is metadata. Ticks before it were completed. This tick was not.
     */
    private static void writeAbort(PrintWriter out, int tick, Throwable cause) {
        String message = cause.getMessage();
        out.print("# abort\t");
        out.print(tick);
        out.print('\t');
        out.print(TraceFormat.escape(cause.getClass().getName()));
        out.print('\t');
        out.print(TraceFormat.escape(message == null || message.isEmpty() ? "-" : message));
        out.print('\n');
        out.flush();
    }

    private static Throwable unwrap(Throwable thrown) {
        Throwable current = thrown;
        while (current instanceof InvocationTargetException && current.getCause() != null) {
            current = current.getCause();
        }
        return current;
    }

    @SuppressWarnings("unchecked")
    private static List<Mascot> mascots(Manager manager, Field mascotsField) throws IllegalAccessException {
        return (List<Mascot>) mascotsField.get(manager);
    }

    private static void writeTick(PrintWriter out, int tick, List<Mascot> mascots) {
        long draws = SeededRandom.draws();
        for (Mascot mascot : mascots) {
            Behavior behavior = mascot.getBehavior();
            String behaviorName = behavior instanceof UserBehavior user ? user.getName()
                    : behavior == null ? "-" : behavior.getClass().getSimpleName();
            out.print("T\t");
            out.print(tick);
            out.print('\t');
            out.print(TraceFormat.escape(mascot.toString()));
            out.print('\t');
            out.print(TraceFormat.escape(behaviorName));
            out.print('\t');
            out.print(TraceFormat.escape(actionName(behavior)));
            out.print('\t');
            out.print(mascot.getAnchor().x);
            out.print('\t');
            out.print(mascot.getAnchor().y);
            out.print('\t');
            out.print(TraceFormat.escape(TraceFormat.imagePath(mascot.getImageKey())));
            out.print('\t');
            out.print(mascot.isLookRight());
            out.print('\t');
            out.print(draws);
            out.print('\n');
        }
    }

    private static String actionName(Behavior behavior) {
        if (!(behavior instanceof UserBehavior)) {
            return behavior == null ? "-" : behavior.getClass().getSimpleName();
        }
        try {
            Field actionField = UserBehavior.class.getDeclaredField("action");
            actionField.setAccessible(true);
            Action action = innermost((Action) actionField.get(behavior));
            if (!(action instanceof ActionBase base)) {
                return action == null ? "-" : action.getClass().getSimpleName();
            }
            Method getSchema = ActionBase.class.getDeclaredMethod("getSchema");
            getSchema.setAccessible(true);
            ResourceBundle schema = (ResourceBundle) getSchema.invoke(base);
            Method getVariables = ActionBase.class.getDeclaredMethod("getVariables");
            getVariables.setAccessible(true);
            VariableMap variables = (VariableMap) getVariables.invoke(base);
            Variable variable = variables.getRawMap().get(schema.getString("Name"));
            if (variable instanceof Constant) {
                String text = variable.toString();
                return text == null || text.isEmpty() ? "-" : text;
            }
            return action.getClass().getSimpleName();
        } catch (ReflectiveOperationException e) {
            return behavior.getClass().getSimpleName();
        }
    }

    private static Action innermost(Action action) throws ReflectiveOperationException {
        Method getAction = ComplexAction.class.getDeclaredMethod("getAction");
        getAction.setAccessible(true);
        while (action instanceof ComplexAction) {
            try {
                Action next = (Action) getAction.invoke(action);
                if (next == null || next == action) {
                    break;
                }
                action = next;
            } catch (InvocationTargetException e) {
                break;
            }
        }
        return action;
    }

    @SuppressWarnings("unchecked")
    private static void injectMouse(Manager manager, Field mascotsField, List<Scenario.MouseSpec> events) throws Exception {
        if (events.isEmpty()) {
            return;
        }
        List<Mascot> live = mascots(manager, mascotsField);
        Method pressed = Mascot.class.getDeclaredMethod("mousePressed", MouseEvent.class);
        Method released = Mascot.class.getDeclaredMethod("mouseReleased", MouseEvent.class);
        pressed.setAccessible(true);
        released.setAccessible(true);
        for (Scenario.MouseSpec event : events) {
            Mascot target = findMascot(live, event.mascot());
            if (target == null) {
                continue;
            }
            boolean right = "right".equals(event.button());
            boolean release = "release".equals(event.type());
            int button = right ? MouseEvent.BUTTON3 : MouseEvent.BUTTON1;
            int id = release ? MouseEvent.MOUSE_RELEASED : MouseEvent.MOUSE_PRESSED;
            MouseEvent awt = new MouseEvent(EVENT_SOURCE, id, 0L, 0, event.x(), event.y(), 1, right, button);
            try {
                (release ? released : pressed).invoke(target, awt);
            } catch (InvocationTargetException e) {
                if (e.getCause() instanceof Exception cause) {
                    throw cause;
                }
                throw e;
            }
        }
    }

    private static Mascot findMascot(List<Mascot> live, String id) {
        if (live.isEmpty()) {
            return null;
        }
        if (id == null) {
            return live.get(0);
        }
        for (Mascot mascot : live) {
            if (mascot.toString().equals(id)) {
                return mascot;
            }
        }
        return null;
    }

    private static NashornScriptEngine nashorn() throws ReflectiveOperationException {
        Field field = Script.class.getDeclaredField("ENGINE");
        field.setAccessible(true);
        return (NashornScriptEngine) field.get(null);
    }

    private static void installRandom(NashornScriptEngine engine) throws javax.script.ScriptException {
        engine.getBindings(ScriptContext.ENGINE_SCOPE).put("__shimeji2Random", SeededRandom.JS);
        // Interpreted eval honors this assignment. Compiled pack scripts do not: Nashorn emits
        // a call to its Math builtin. Script.compile rewrites those calls. See apply-patches.py.
        engine.eval("Math.random = __shimeji2Random;");
    }

    private static void probeRandom(NashornScriptEngine engine) throws javax.script.ScriptException {
        SeededRandom.reseed(PROBE_SEED);
        Random mirror = new Random(PROBE_SEED);
        double first = ((Number) engine.eval("Math.random()")).doubleValue();
        double second = ((Number) engine.eval("Math.random()")).doubleValue();
        com.group_finity.mascot.script.VariableMap variables = new com.group_finity.mascot.script.VariableMap();
        variables.put("__shimeji2Random", SeededRandom.JS);
        javax.script.CompiledScript compiled = engine.compile("__shimeji2Random()");
        double third = ((Number) compiled.eval(variables)).doubleValue();
        double expectedFirst = mirror.nextDouble();
        double expectedSecond = mirror.nextDouble();
        double expectedThird = mirror.nextDouble();
        if (Double.doubleToRawLongBits(first) != Double.doubleToRawLongBits(expectedFirst)
                || Double.doubleToRawLongBits(second) != Double.doubleToRawLongBits(expectedSecond)
                || Double.doubleToRawLongBits(third) != Double.doubleToRawLongBits(expectedThird)) {
            throw new IllegalStateException("Nashorn Math.random is not the seeded stream ("
                    + first + ", " + second + ", " + third + " vs "
                    + expectedFirst + ", " + expectedSecond + ", " + expectedThird + ")");
        }
    }

    private static Object invoke(Object target, String name, Class<?>[] types, Object... values) throws Exception {
        Method method = target.getClass().getDeclaredMethod(name, types);
        method.setAccessible(true);
        try {
            return method.invoke(target, values);
        } catch (InvocationTargetException e) {
            if (e.getCause() instanceof Exception cause) {
                throw cause;
            }
            throw e;
        }
    }

    private record Args(Path scenario, Path out, boolean verbose, int ticks) {
        static Args parse(String[] args) {
            Path scenario = null;
            Path out = null;
            boolean verbose = false;
            int ticks = 0;
            for (int i = 0; i < args.length; i++) {
                switch (args[i]) {
                    case "--scenario" -> scenario = Path.of(args[++i]);
                    case "--out" -> out = Path.of(args[++i]);
                    case "--verbose" -> verbose = true;
                    case "--ticks" -> ticks = Integer.parseInt(args[++i]);
                    default -> throw new IllegalArgumentException("unknown argument " + args[i]);
                }
            }
            if (scenario == null || out == null) {
                throw new IllegalArgumentException("usage: TraceRunner --scenario FILE --out FILE [--ticks N] [--verbose]");
            }
            return new Args(scenario, out, verbose, ticks);
        }
    }
}
