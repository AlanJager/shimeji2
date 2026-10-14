package shimeji2.ref;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/**
 * Fails the harness if any engine class still calls {@code java.lang.Math.random}
 * or constructs a {@code java.util.Random}.
 */
public final class RngScan {
    private RngScan() {
    }

    public static void scanEngineClasses() throws IOException {
        var codeSource = com.group_finity.mascot.Main.class.getProtectionDomain().getCodeSource();
        if (codeSource == null || codeSource.getLocation() == null) {
            throw new IllegalStateException("engine code source is missing");
        }
        Path root;
        try {
            root = Path.of(codeSource.getLocation().toURI());
        } catch (Exception e) {
            throw new IllegalStateException("engine code source is not a directory: " + codeSource.getLocation(), e);
        }
        if (!Files.isDirectory(root)) {
            throw new IllegalStateException("RNG scan expects a classes directory, got " + root);
        }
        Path pkg = root.resolve("com/group_finity");
        List<String> hits = new ArrayList<>();
        try (var walk = Files.walk(pkg)) {
            for (Path classFile : walk.filter(path -> path.toString().endsWith(".class")).toList()) {
                scan(classFile, root.relativize(classFile).toString(), hits);
            }
        }
        if (!hits.isEmpty()) {
            throw new IllegalStateException("RNG scan failed:\n" + String.join("\n", hits));
        }
    }

    private static void scan(Path classFile, String name, List<String> hits) throws IOException {
        byte[] bytes = Files.readAllBytes(classFile);
        List<Cp> pool = constantPool(bytes);
        for (Cp entry : pool) {
            if (entry == null || entry.tag != 10) {
                continue;
            }
            Cp classEntry = pool.get(entry.a);
            String className = utf(pool, classEntry.a);
            Cp nameAndType = pool.get(entry.b);
            String method = utf(pool, nameAndType.a);
            if ("java/lang/Math".equals(className) && "random".equals(method)) {
                hits.add(name + " still calls java/lang/Math.random");
            }
            if ("java/util/Random".equals(className) && "<init>".equals(method)) {
                hits.add(name + " still constructs java/util/Random");
            }
        }
    }

    private static String utf(List<Cp> pool, int index) {
        Cp entry = pool.get(index);
        return entry.utf;
    }

    private static List<Cp> constantPool(byte[] bytes) {
        if (bytes.length < 10 || (bytes[0] & 0xff) != 0xca || (bytes[1] & 0xff) != 0xfe) {
            throw new IllegalStateException("not a class file");
        }
        int count = u2(bytes, 8);
        List<Cp> pool = new ArrayList<>(count);
        for (int i = 0; i < count; i++) {
            pool.add(null);
        }
        int offset = 10;
        for (int i = 1; i < count; i++) {
            int tag = bytes[offset] & 0xff;
            offset++;
            Cp entry = new Cp(tag);
            switch (tag) {
                case 1 -> {
                    int len = u2(bytes, offset);
                    offset += 2;
                    entry.utf = new String(bytes, offset, len, java.nio.charset.StandardCharsets.UTF_8);
                    offset += len;
                }
                case 7, 8, 16, 19, 20 -> {
                    entry.a = u2(bytes, offset);
                    offset += 2;
                }
                case 15 -> offset += 3;
                case 3, 4, 9, 10, 11, 12, 17, 18 -> {
                    entry.a = u2(bytes, offset);
                    entry.b = u2(bytes, offset + 2);
                    offset += 4;
                }
                case 5, 6 -> {
                    offset += 8;
                    pool.set(i, entry);
                    i++;
                    continue;
                }
                default -> throw new IllegalStateException("unknown constant-pool tag " + tag);
            }
            pool.set(i, entry);
        }
        return pool;
    }

    private static int u2(byte[] bytes, int offset) {
        return ((bytes[offset] & 0xff) << 8) | (bytes[offset + 1] & 0xff);
    }

    private static final class Cp {
        final int tag;
        int a;
        int b;
        String utf = "";

        Cp(int tag) {
            this.tag = tag;
        }
    }

}
