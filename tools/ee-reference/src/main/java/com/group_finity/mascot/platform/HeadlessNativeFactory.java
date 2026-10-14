package com.group_finity.mascot.platform;

import com.group_finity.mascot.environment.Environment;

/**
 * Platform factory used when {@code -Dshimeji2.ref.factory=headless}.
 * The instance is created from the patched {@link NativeFactory#resetInstance()}.
 */
public final class HeadlessNativeFactory extends NativeFactory {
    private final Environment environment;

    public HeadlessNativeFactory() {
        environment = shimeji2.ref.HeadlessHolder.environment;
        if (environment == null) {
            throw new IllegalStateException("HeadlessHolder.environment was not set before NativeFactory init");
        }
    }

    @Override
    public Environment getEnvironment() {
        return environment;
    }

    @Override
    public TranslucentWindow newTranslucentWindow() {
        return new shimeji2.ref.HeadlessWindow();
    }
}
