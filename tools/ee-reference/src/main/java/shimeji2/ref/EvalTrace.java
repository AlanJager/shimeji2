package shimeji2.ref;

import com.group_finity.mascot.script.Constant;
import com.group_finity.mascot.script.Variable;
import com.group_finity.mascot.script.VariableMap;

import java.io.PrintWriter;

/**
 * Opt-in log of Nashorn evaluations. The patched {@code Script.get} calls {@link #record}.
 * When disabled, the method returns before touching the variable map.
 */
public final class EvalTrace {
    public static volatile boolean enabled;
    public static volatile int tick = -1;
    public static PrintWriter out;

    private EvalTrace() {
    }

    public static void record(String source, Object value, VariableMap variables) {
        if (!enabled || out == null) {
            return;
        }
        String mascot = "-";
        if (variables != null) {
            Variable variable = variables.getRawMap().get("mascot");
            if (variable instanceof Constant) {
                mascot = variable.toString();
            }
        }
        out.print("E\t");
        out.print(tick);
        out.print('\t');
        out.print(TraceFormat.escape(mascot));
        out.print('\t');
        out.print(TraceFormat.escape(source));
        out.print('\t');
        out.print(TraceFormat.formatValue(value));
        out.print('\n');
    }
}
