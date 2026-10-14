package shimeji2.ref;

import org.openjdk.nashorn.api.scripting.AbstractJSObject;

import java.util.Random;

/**
 * The one random stream shared by patched engine call sites and Nashorn {@code Math.random}.
 * {@link #random()} matches {@code java.lang.Math#random()}: one {@link Random#nextDouble()}.
 */
public final class SeededRandom {
    /**
     * Nashorn function object. Compiled pack scripts cannot see ENGINE_SCOPE, and a compiled
     * {@code Math.random} call ignores a global assignment, so {@code Script} rewrites the call
     * to {@code __shimeji2Random()} and places this object in the variable map.
     */
    public static final AbstractJSObject JS = new AbstractJSObject() {
        @Override
        public boolean isFunction() {
            return true;
        }

        @Override
        public Object call(Object thiz, Object... args) {
            return random();
        }

        @Override
        public Object getDefaultValue(Class<?> hint) {
            // Nashorn's real Math.random function converts to NaN. It does not draw.
            // shime's walk target uses `Math.random*100` with no call on one branch.
            if (hint == String.class) {
                return "function random() { [native code] }";
            }
            return Double.NaN;
        }
    };

    private static Random random = new Random(1);
    private static long draws;

    private SeededRandom() {
    }

    public static synchronized void reseed(long seed) {
        random = new Random(seed);
        draws = 0;
    }

    public static synchronized double random() {
        draws++;
        return random.nextDouble();
    }

    public static synchronized long draws() {
        return draws;
    }
}
