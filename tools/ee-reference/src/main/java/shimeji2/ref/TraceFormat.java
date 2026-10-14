package shimeji2.ref;

/**
 * Tab-separated trace fields. See {@code tests/scenarios/schema.md}.
 */
public final class TraceFormat {
    private TraceFormat() {
    }

    public static String escape(String text) {
        if (text == null) {
            return "-";
        }
        StringBuilder out = new StringBuilder(text.length());
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case '\\' -> out.append("\\\\");
                case '\t' -> out.append("\\t");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                default -> out.append(c);
            }
        }
        return out.toString();
    }

    public static String formatValue(Object value) {
        if (value == null) {
            return "null";
        }
        String className = value.getClass().getName();
        if (className.endsWith("Undefined")) {
            return "undefined";
        }
        if (value instanceof Boolean || value instanceof Number) {
            return escape(value.toString());
        }
        if (value instanceof String text) {
            return escape("\"" + text + "\"");
        }
        if (value instanceof com.group_finity.mascot.Mascot) {
            return escape(value.toString());
        }
        return escape("obj:" + className);
    }

    /**
     * XML image path ({@code /shime1.png}) from an {@code ImagePairs} key
     * ({@code 64,128:shime//shime1.png}).
     */
    public static String imagePath(String imageKey) {
        if (imageKey == null || imageKey.isEmpty()) {
            return "-";
        }
        int colon = imageKey.indexOf(':');
        String rest = colon >= 0 ? imageKey.substring(colon + 1) : imageKey;
        int slash = rest.lastIndexOf('/');
        String file = slash >= 0 ? rest.substring(slash) : "/" + rest;
        if (!file.startsWith("/")) {
            file = "/" + file;
        }
        return file;
    }
}
