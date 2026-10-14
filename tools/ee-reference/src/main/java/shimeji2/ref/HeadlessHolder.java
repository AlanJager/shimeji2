package shimeji2.ref;

/**
 * Set before {@code NativeFactory} initializes. The patched factory reads this.
 */
public final class HeadlessHolder {
    public static HeadlessEnvironment environment;

    private HeadlessHolder() {
    }
}
