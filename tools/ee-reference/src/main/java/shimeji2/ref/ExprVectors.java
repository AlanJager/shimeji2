package shimeji2.ref;

import org.openjdk.nashorn.api.scripting.AbstractJSObject;
import org.openjdk.nashorn.api.scripting.NashornScriptEngine;
import org.openjdk.nashorn.api.scripting.NashornScriptEngineFactory;

import javax.script.Bindings;
import javax.script.CompiledScript;
import javax.script.ScriptException;
import javax.script.SimpleBindings;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;

/**
 * Captures {@code java.util.Random} sequences and Nashorn evaluation results for Unit 4.
 * Compiled scripts rewrite {@code Math.random} the same way {@code Script} does.
 */
public final class ExprVectors {
    private static final long EXPR_SEED = 42L;
    private static final int SEQUENCE_LEN = 10;

    private ExprVectors() {
    }

    public static void main(String[] args) {
        try {
            run(args);
        } catch (Throwable failure) {
            failure.printStackTrace(System.err);
            System.exit(1);
        }
    }

    private static void run(String[] args) throws Exception {
        Path shimeConf = null;
        Path outNashorn = null;
        Path outRandom = null;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--shime-conf" -> shimeConf = Path.of(args[++i]);
                case "--out-nashorn" -> outNashorn = Path.of(args[++i]);
                case "--out-random" -> outRandom = Path.of(args[++i]);
                default -> throw new IllegalArgumentException("unknown arg: " + args[i]);
            }
        }
        if (outNashorn == null || outRandom == null) {
            throw new IllegalArgumentException("need --out-nashorn and --out-random");
        }

        writeRandom(outRandom);
        writeNashorn(outNashorn, shimeConf);
    }

    private static void writeRandom(Path out) throws Exception {
        Map<String, Object> root = new LinkedHashMap<>();
        root.put("source", "java.util.Random");
        root.put("jdk", System.getProperty("java.version"));
        root.put("note", "Each named sequence is taken from a fresh Random(seed). "
                + "nextDouble entries include IEEE-754 bits so a port can match bit-exactly.");
        List<Object> seeds = new ArrayList<>();
        for (long seed : new long[]{42L, 1L}) {
            Map<String, Object> block = new LinkedHashMap<>();
            block.put("seed", seed);
            block.put("nextDouble", doubles(new Random(seed)));
            block.put("nextInt_100", intsBound(new Random(seed), 100));
            block.put("nextInt", ints(new Random(seed)));
            block.put("nextInt_8", intsBound(new Random(seed), 8));
            block.put("nextInt_3", intsBound(new Random(seed), 3));
            block.put("nextBoolean", bools(new Random(seed)));
            seeds.add(block);
        }
        root.put("seeds", seeds);
        Files.createDirectories(out.getParent());
        Files.writeString(out, JsonWriter.write(root) + "\n", StandardCharsets.UTF_8);
    }

    private static List<Object> doubles(Random random) {
        List<Object> values = new ArrayList<>();
        for (int i = 0; i < SEQUENCE_LEN; i++) {
            double value = random.nextDouble();
            Map<String, Object> entry = new LinkedHashMap<>();
            entry.put("decimal", Double.toString(value));
            entry.put("bits", bits(value));
            values.add(entry);
        }
        return values;
    }

    private static List<Integer> ints(Random random) {
        List<Integer> values = new ArrayList<>();
        for (int i = 0; i < SEQUENCE_LEN; i++) {
            values.add(random.nextInt());
        }
        return values;
    }

    private static List<Integer> intsBound(Random random, int bound) {
        List<Integer> values = new ArrayList<>();
        for (int i = 0; i < SEQUENCE_LEN; i++) {
            values.add(random.nextInt(bound));
        }
        return values;
    }

    private static List<Boolean> bools(Random random) {
        List<Boolean> values = new ArrayList<>();
        for (int i = 0; i < SEQUENCE_LEN; i++) {
            values.add(random.nextBoolean());
        }
        return values;
    }

    private static String bits(double value) {
        return String.format("%016x", Double.doubleToRawLongBits(value));
    }

    private static void writeNashorn(Path out, Path shimeConf) throws Exception {
        NashornScriptEngineFactory factory = new NashornScriptEngineFactory();
        NashornScriptEngine engine = (NashornScriptEngine) factory.getScriptEngine(className -> false);
        Object undefined = engine.eval("undefined");

        StubMascot mascot = StubMascot.fixed();
        Map<String, Object> stub = stubDescription();

        List<Object> cases = new ArrayList<>();
        for (String[] pair : SEMANTIC_CASES) {
            cases.add(evalCase(engine, undefined, mascot, pair[0], pair[1], "semantics"));
        }
        if (shimeConf != null) {
            int index = 0;
            for (String expr : extractXmlScripts(shimeConf)) {
                cases.add(evalCase(engine, undefined, mascot, "shime-" + index, expr, "shime"));
                index++;
            }
        }

        String nashornVersion = engine.getClass().getPackage().getImplementationVersion();
        if (nashornVersion == null || nashornVersion.isBlank()) {
            nashornVersion = "15.7";
        }

        Map<String, Object> root = new LinkedHashMap<>();
        root.put("engine", engine.getClass().getName());
        root.put("nashorn", nashornVersion);
        root.put("jdk", System.getProperty("java.version"));
        root.put("seed", EXPR_SEED);
        root.put("note", "Each case reseeds SeededRandom to seed. Math.random is rewritten to "
                + "__shimeji2Random at compile time, matching Script.java. A bare Math.random "
                + "(no call) converts to NaN and does not draw.");
        root.put("stub", stub);
        root.put("cases", cases);

        Files.createDirectories(out.getParent());
        Files.writeString(out, JsonWriter.write(root) + "\n", StandardCharsets.UTF_8);
    }

    private static Map<String, Object> evalCase(
            NashornScriptEngine engine,
            Object undefined,
            StubMascot mascot,
            String id,
            String expr,
            String group
    ) {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("id", id);
        result.put("group", group);
        result.put("expr", expr);
        SeededRandom.reseed(EXPR_SEED);
        long before = SeededRandom.draws();
        try {
            CompiledScript compiled = engine.compile(expr.replace("Math.random", "__shimeji2Random"));
            Object value = compiled.eval(bindings(undefined, mascot));
            result.put("ok", true);
            putValue(result, value);
        } catch (ScriptException error) {
            result.put("ok", false);
            result.put("error", error.getMessage());
        }
        result.put("draws", SeededRandom.draws() - before);
        return result;
    }

    private static Bindings bindings(Object undefined, StubMascot mascot) {
        SimpleBindings bindings = new SimpleBindings();
        bindings.put("mascot", mascot);
        bindings.put("Math", MATH);
        bindings.put("__shimeji2Random", SeededRandom.JS);
        bindings.put("undefined", undefined);
        bindings.put("NaN", Double.NaN);
        bindings.put("Infinity", Double.POSITIVE_INFINITY);
        bindings.put("maxCount", "50");
        bindings.put("目的地Y", 150);
        bindings.put("ずれ", 10);
        bindings.put("footX", 90);
        bindings.put("FootX", 90);
        bindings.put("TargetY", 150);
        bindings.put("a", Boolean.TRUE);
        return bindings;
    }

    private static void putValue(Map<String, Object> result, Object value) {
        if (value == null) {
            result.put("type", "null");
            result.put("value", null);
            return;
        }
        String className = value.getClass().getName();
        result.put("javaClass", className);
        if (className.endsWith("Undefined")) {
            result.put("type", "undefined");
            result.put("value", "undefined");
            return;
        }
        if (value instanceof Boolean bool) {
            result.put("type", "boolean");
            result.put("value", bool);
            return;
        }
        if (value instanceof Number number) {
            result.put("type", "number");
            double d = number.doubleValue();
            if (Double.isNaN(d)) {
                result.put("value", "NaN");
            } else if (d == Double.POSITIVE_INFINITY) {
                result.put("value", "Infinity");
            } else if (d == Double.NEGATIVE_INFINITY) {
                result.put("value", "-Infinity");
            } else if (value instanceof Double || value instanceof Float) {
                result.put("value", d);
            } else {
                result.put("value", number);
            }
            result.put("bits", bits(d));
            return;
        }
        if (value instanceof String text) {
            result.put("type", "string");
            result.put("value", text);
            return;
        }
        result.put("type", "object");
        result.put("value", String.valueOf(value));
    }

    private static Map<String, Object> stubDescription() {
        Map<String, Object> stub = new LinkedHashMap<>();
        stub.put("mascot.anchor", Map.of("x", 100, "y", 200));
        stub.put("mascot.lookRight", true);
        stub.put("mascot.totalCount", 3);
        stub.put("mascot.environment.cursor", Map.of("x", 150, "y", 80, "dx", 2, "dy", -1));
        stub.put("mascot.environment.screen", area(0, 0, 1280, 800, true));
        stub.put("mascot.environment.workArea", area(0, 25, 1280, 800, true));
        stub.put("mascot.environment.activeIE", area(200, 100, 600, 400, true));
        stub.put("isOn.floor", true);
        stub.put("isOn.ceiling", false);
        stub.put("isOn.wall", false);
        stub.put("isOn.workArea.leftBorder", false);
        stub.put("isOn.workArea.rightBorder", false);
        stub.put("isOn.workArea.topBorder", false);
        stub.put("isOn.workArea.bottomBorder", true);
        stub.put("isOn.activeIE.leftBorder", false);
        stub.put("isOn.activeIE.rightBorder", false);
        stub.put("isOn.activeIE.topBorder", false);
        stub.put("isOn.activeIE.bottomBorder", false);
        stub.put("maxCount", "50");
        stub.put("目的地Y", 150);
        stub.put("ずれ", 10);
        stub.put("footX", 90);
        stub.put("a", true);
        return stub;
    }

    private static Map<String, Object> area(int left, int top, int right, int bottom, boolean visible) {
        Map<String, Object> map = new LinkedHashMap<>();
        map.put("left", left);
        map.put("top", top);
        map.put("right", right);
        map.put("bottom", bottom);
        map.put("width", right - left);
        map.put("height", bottom - top);
        map.put("visible", visible);
        return map;
    }

    private static List<String> extractXmlScripts(Path confDir) throws Exception {
        List<String> found = new ArrayList<>();
        List<String> seen = new ArrayList<>();
        try (var stream = Files.list(confDir)) {
            List<Path> files = stream.filter(path -> path.getFileName().toString().endsWith(".xml"))
                    .sorted()
                    .toList();
            for (Path file : files) {
                String text = stripComments(Files.readString(file, StandardCharsets.UTF_8));
                int index = 0;
                while (index < text.length()) {
                    int dollar = text.indexOf("${", index);
                    int hash = text.indexOf("#{", index);
                    int start;
                    if (dollar < 0 && hash < 0) {
                        break;
                    }
                    if (dollar < 0) {
                        start = hash;
                    } else if (hash < 0) {
                        start = dollar;
                    } else {
                        start = Math.min(dollar, hash);
                    }
                    int cursor = start + 2;
                    int depth = 1;
                    while (cursor < text.length() && depth > 0) {
                        char c = text.charAt(cursor);
                        if (c == '{') {
                            depth++;
                        } else if (c == '}') {
                            depth--;
                        }
                        cursor++;
                    }
                    if (depth != 0) {
                        break;
                    }
                    String interior = unescapeXml(text.substring(start + 2, cursor - 1)).strip();
                    String key = interior.replaceAll("\\s+", " ");
                    if (!key.isEmpty() && !seen.contains(key)) {
                        seen.add(key);
                        found.add(interior);
                    }
                    index = cursor;
                }
            }
        }
        return found;
    }

    private static String stripComments(String text) {
        StringBuilder out = new StringBuilder(text.length());
        int index = 0;
        while (index < text.length()) {
            int comment = text.indexOf("<!--", index);
            if (comment < 0) {
                out.append(text, index, text.length());
                break;
            }
            out.append(text, index, comment);
            int end = text.indexOf("-->", comment + 4);
            if (end < 0) {
                break;
            }
            index = end + 3;
        }
        return out.toString();
    }

    private static String unescapeXml(String text) {
        StringBuilder out = new StringBuilder(text.length());
        int index = 0;
        while (index < text.length()) {
            int amp = text.indexOf('&', index);
            if (amp < 0) {
                out.append(text, index, text.length());
                break;
            }
            out.append(text, index, amp);
            int semi = text.indexOf(';', amp + 1);
            if (semi < 0) {
                out.append(text, amp, text.length());
                break;
            }
            String entity = text.substring(amp + 1, semi);
            switch (entity) {
                case "lt" -> out.append('<');
                case "gt" -> out.append('>');
                case "amp" -> out.append('&');
                case "quot" -> out.append('"');
                case "apos" -> out.append('\'');
                default -> {
                    if (entity.startsWith("#x") || entity.startsWith("#X")) {
                        out.append((char) Integer.parseInt(entity.substring(2), 16));
                    } else if (entity.startsWith("#")) {
                        out.append((char) Integer.parseInt(entity.substring(1)));
                    } else {
                        out.append('&').append(entity).append(';');
                    }
                }
            }
            index = semi + 1;
        }
        return out.toString();
    }

    private static final AbstractJSObject MATH = new AbstractJSObject() {
        private final Map<String, AbstractJSObject> members = Map.of(
                "abs", new MathFn("abs"),
                "min", new MathFn("min"),
                "max", new MathFn("max"),
                "floor", new MathFn("floor"),
                "ceil", new MathFn("ceil"),
                "round", new MathFn("round"),
                "random", SeededRandom.JS
        );

        @Override
        public boolean hasMember(String name) {
            return members.containsKey(name);
        }

        @Override
        public Object getMember(String name) {
            return members.get(name);
        }
    };

    private static final class MathFn extends AbstractJSObject {
        private final String name;

        private MathFn(String name) {
            this.name = name;
        }

        @Override
        public boolean isFunction() {
            return true;
        }

        @Override
        public Object call(Object thiz, Object... args) {
            return switch (name) {
                case "abs" -> Math.abs(num(args, 0));
                case "floor" -> Math.floor(num(args, 0));
                case "ceil" -> Math.ceil(num(args, 0));
                case "round" -> (double) Math.round(num(args, 0));
                case "min" -> {
                    double acc = Double.POSITIVE_INFINITY;
                    for (Object arg : args) {
                        acc = Math.min(acc, toDouble(arg));
                    }
                    yield acc;
                }
                case "max" -> {
                    double acc = Double.NEGATIVE_INFINITY;
                    for (Object arg : args) {
                        acc = Math.max(acc, toDouble(arg));
                    }
                    yield acc;
                }
                default -> throw new IllegalStateException(name);
            };
        }

        private static double num(Object[] args, int index) {
            if (args == null || index >= args.length) {
                return Double.NaN;
            }
            return toDouble(args[index]);
        }
    }

    private static double toDouble(Object value) {
        if (value == null) {
            return 0.0;
        }
        if (value instanceof Number number) {
            return number.doubleValue();
        }
        if (value instanceof Boolean bool) {
            return bool ? 1.0 : 0.0;
        }
        if (value instanceof String text) {
            try {
                return Double.parseDouble(text);
            } catch (NumberFormatException ignored) {
                return Double.NaN;
            }
        }
        String className = value.getClass().getName();
        if (className.endsWith("Undefined")) {
            return Double.NaN;
        }
        return Double.NaN;
    }

    public static final class StubPoint {
        private final int x;
        private final int y;
        private final int dx;
        private final int dy;

        StubPoint(int x, int y, int dx, int dy) {
            this.x = x;
            this.y = y;
            this.dx = dx;
            this.dy = dy;
        }

        public int getX() {
            return x;
        }

        public int getY() {
            return y;
        }

        public int getDx() {
            return dx;
        }

        public int getDy() {
            return dy;
        }
    }

    public static final class StubBorder {
        private final boolean on;

        StubBorder(boolean on) {
            this.on = on;
        }

        public boolean isOn(Object point) {
            return on;
        }
    }

    public static final class StubArea {
        private final int left;
        private final int top;
        private final int right;
        private final int bottom;
        private final boolean visible;
        private final StubBorder leftBorder;
        private final StubBorder rightBorder;
        private final StubBorder topBorder;
        private final StubBorder bottomBorder;

        StubArea(int left, int top, int right, int bottom, boolean visible,
                 boolean leftOn, boolean rightOn, boolean topOn, boolean bottomOn) {
            this.left = left;
            this.top = top;
            this.right = right;
            this.bottom = bottom;
            this.visible = visible;
            this.leftBorder = new StubBorder(leftOn);
            this.rightBorder = new StubBorder(rightOn);
            this.topBorder = new StubBorder(topOn);
            this.bottomBorder = new StubBorder(bottomOn);
        }

        public int getLeft() {
            return left;
        }

        public int getTop() {
            return top;
        }

        public int getRight() {
            return right;
        }

        public int getBottom() {
            return bottom;
        }

        public int getWidth() {
            return right - left;
        }

        public int getHeight() {
            return bottom - top;
        }

        public boolean isVisible() {
            return visible;
        }

        public boolean getVisible() {
            return visible;
        }

        public StubBorder getLeftBorder() {
            return leftBorder;
        }

        public StubBorder getRightBorder() {
            return rightBorder;
        }

        public StubBorder getTopBorder() {
            return topBorder;
        }

        public StubBorder getBottomBorder() {
            return bottomBorder;
        }
    }

    public static final class StubEnv {
        private final StubPoint cursor;
        private final StubArea screen;
        private final StubArea workArea;
        private final StubArea activeIE;
        private final StubBorder floor;
        private final StubBorder ceiling;
        private final StubBorder wall;

        StubEnv(StubPoint cursor, StubArea screen, StubArea workArea, StubArea activeIE,
                StubBorder floor, StubBorder ceiling, StubBorder wall) {
            this.cursor = cursor;
            this.screen = screen;
            this.workArea = workArea;
            this.activeIE = activeIE;
            this.floor = floor;
            this.ceiling = ceiling;
            this.wall = wall;
        }

        public StubPoint getCursor() {
            return cursor;
        }

        public StubArea getScreen() {
            return screen;
        }

        public StubArea getWorkArea() {
            return workArea;
        }

        public StubArea getActiveIE() {
            return activeIE;
        }

        public StubBorder getFloor() {
            return floor;
        }

        public StubBorder getCeiling() {
            return ceiling;
        }

        public StubBorder getWall() {
            return wall;
        }
    }

    public static final class StubMascot {
        private final StubPoint anchor;
        private final boolean lookRight;
        private final int totalCount;
        private final StubEnv environment;

        StubMascot(StubPoint anchor, boolean lookRight, int totalCount, StubEnv environment) {
            this.anchor = anchor;
            this.lookRight = lookRight;
            this.totalCount = totalCount;
            this.environment = environment;
        }

        static StubMascot fixed() {
            StubPoint anchor = new StubPoint(100, 200, 0, 0);
            StubPoint cursor = new StubPoint(150, 80, 2, -1);
            StubArea screen = new StubArea(0, 0, 1280, 800, true, false, false, false, false);
            StubArea workArea = new StubArea(0, 25, 1280, 800, true, false, false, false, true);
            StubArea activeIE = new StubArea(200, 100, 600, 400, true, false, false, false, false);
            StubEnv env = new StubEnv(
                    cursor,
                    screen,
                    workArea,
                    activeIE,
                    new StubBorder(true),
                    new StubBorder(false),
                    new StubBorder(false)
            );
            return new StubMascot(anchor, true, 3, env);
        }

        public StubPoint getAnchor() {
            return anchor;
        }

        public boolean isLookRight() {
            return lookRight;
        }

        public boolean getLookRight() {
            return lookRight;
        }

        public int getTotalCount() {
            return totalCount;
        }

        public StubEnv getEnvironment() {
            return environment;
        }
    }

    /**
     * Hand-written coercion and semantics cases. Bundled shime expressions are appended from XML.
     * Corpus expressions are not included.
     */
    private static final String[][] SEMANTIC_CASES = {
            {"string-concat-left", "\"5\" + 1"},
            {"string-concat-right", "1 + \"5\""},
            {"maxCount-lt-number", "maxCount < 50"},
            {"maxCount-lt-51", "maxCount < 51"},
            {"totalCount-lt-maxCount", "mascot.totalCount < maxCount"},
            {"bare-math-random-times", "Math.random*100"},
            {"called-math-random-times", "Math.random() * 100"},
            {"short-circuit-true", "a || Math.random() < 0.5"},
            {"short-circuit-false", "false || Math.random() < 0.5"},
            {"ternary-string", "true ? \"yes\" : 0"},
            {"ternary-else-number", "false ? \"yes\" : 0"},
            {"ternary-then-number", "true ? 1 : \"no\""},
            {"div-by-zero", "1/0"},
            {"zero-div-zero", "0/0"},
            {"neg-div-zero", "-1/0"},
            {"null-plus", "null + 1"},
            {"undefined-plus", "undefined + 1"},
            {"null-times", "null * 2"},
            {"undefined-times", "undefined * 2"},
            {"japanese-ident", "目的地Y"},
            {"japanese-zure", "ずれ + 1"},
            {"not-false", "!false"},
            {"not-true", "!true"},
            {"not-zero", "!0"},
            {"not-one", "!1"},
            {"loose-eq-string-number", "1 == \"1\""},
            {"strict-eq-string-number", "1 === \"1\""},
            {"loose-ne", "1 != \"1\""},
            {"strict-ne", "1 !== \"1\""},
            {"math-abs", "Math.abs(-3)"},
            {"math-min", "Math.min(2, 5)"},
            {"math-max", "Math.max(2, 5)"},
            {"math-floor", "Math.floor(1.9)"},
            {"math-ceil", "Math.ceil(1.1)"},
            {"math-round", "Math.round(1.5)"},
            {"math-round-nan", "Math.round(NaN)"},
            {"math-round-negative-fraction", "Math.round(-0.4)"},
            {"math-round-half-up", "Math.round(2.5)"},
            {"math-round-negative-half", "Math.round(-2.5)"},
            {"unary-plus-string", "+\"5\""},
            {"unary-plus-then-add", "+\"5\" + 1"},
            {"and-numbers", "1 && 2"},
            {"and-short-circuit-random", "0 && Math.random()"},
            {"null-eq-undefined", "null == undefined"},
            {"null-seq-undefined", "null === undefined"},
            {"happy-path-cursor-lerp",
                    "mascot.anchor.x+(mascot.environment.cursor.x-mascot.anchor.x)*Math.random()"},
            {"not-floor-isOn", "!mascot.environment.floor.isOn(mascot.anchor)"},
            {"lookRight-ternary", "mascot.lookRight ? 1 : 0"},
            {"mod", "5 % 2"},
            {"div", "5 / 2"},
            {"nan-ident", "NaN"},
            {"infinity-ident", "Infinity"},
            {"unknown-ident", "notAThing"},
    };

    private static final class JsonWriter {
        private JsonWriter() {
        }

        static String write(Object value) {
            StringBuilder out = new StringBuilder();
            write(out, value, 0);
            return out.toString();
        }

        private static void write(StringBuilder out, Object value, int indent) {
            if (value == null) {
                out.append("null");
            } else if (value instanceof Boolean || value instanceof Integer || value instanceof Long) {
                out.append(value);
            } else if (value instanceof Double d) {
                if (Double.isFinite(d)) {
                    out.append(d);
                } else if (Double.isNaN(d)) {
                    quote(out, "NaN");
                } else if (d > 0) {
                    quote(out, "Infinity");
                } else {
                    quote(out, "-Infinity");
                }
            } else if (value instanceof Number number) {
                out.append(number);
            } else if (value instanceof String text) {
                quote(out, text);
            } else if (value instanceof List<?> list) {
                if (list.isEmpty()) {
                    out.append("[]");
                    return;
                }
                out.append("[\n");
                for (int i = 0; i < list.size(); i++) {
                    pad(out, indent + 1);
                    write(out, list.get(i), indent + 1);
                    if (i + 1 < list.size()) {
                        out.append(',');
                    }
                    out.append('\n');
                }
                pad(out, indent);
                out.append(']');
            } else if (value instanceof Map<?, ?> map) {
                if (map.isEmpty()) {
                    out.append("{}");
                    return;
                }
                out.append("{\n");
                int i = 0;
                int size = map.size();
                for (Map.Entry<?, ?> entry : map.entrySet()) {
                    pad(out, indent + 1);
                    quote(out, String.valueOf(entry.getKey()));
                    out.append(": ");
                    write(out, entry.getValue(), indent + 1);
                    i++;
                    if (i < size) {
                        out.append(',');
                    }
                    out.append('\n');
                }
                pad(out, indent);
                out.append('}');
            } else {
                quote(out, String.valueOf(value));
            }
        }

        private static void pad(StringBuilder out, int indent) {
            out.append("  ".repeat(indent));
        }

        private static void quote(StringBuilder out, String text) {
            out.append('"');
            for (int i = 0; i < text.length(); i++) {
                char c = text.charAt(i);
                switch (c) {
                    case '"' -> out.append("\\\"");
                    case '\\' -> out.append("\\\\");
                    case '\n' -> out.append("\\n");
                    case '\r' -> out.append("\\r");
                    case '\t' -> out.append("\\t");
                    default -> {
                        if (c < 0x20) {
                            out.append(String.format("\\u%04x", (int) c));
                        } else {
                            out.append(c);
                        }
                    }
                }
            }
            out.append('"');
        }
    }
}
