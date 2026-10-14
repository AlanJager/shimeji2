package shimeji2.ref;

import com.group_finity.mascot.platform.TranslucentWindow;

import javax.swing.JPanel;
import java.awt.Component;
import java.awt.image.BufferedImage;

/**
 * A parentless panel. The engine still attaches mouse listeners and calls
 * {@code setVisible}, but nothing is shown and nothing is read back from the display.
 */
public final class HeadlessWindow implements TranslucentWindow {
    private final JPanel panel = new JPanel(null);

    public HeadlessWindow() {
        panel.setOpaque(false);
    }

    @Override
    public Component asComponent() {
        return panel;
    }

    @Override
    public void setImage(BufferedImage image) {
    }

    @Override
    public void updateImage() {
    }

    @Override
    public void dispose() {
    }

    @Override
    public void setAlwaysOnTop(boolean onTop) {
    }
}
