package tools.oracle;

import ij.IJ;
import ij.ImageJ;
import ij.macro.Interpreter;

/**
 * Protocol runner for ImageJ oracle macros.
 *
 * Replaces direct ij.ImageJ -eval invocation which unconditionally exited with status 0
 * even when macros were aborted via exit(...) or runtime errors.
 *
 * Protocol contract:
 * - Macro executes cleanly without abortion -> exits with code 0; macro return value (if any) printed to stdout.
 * - Ordinary stdout containing "[aborted]" (e.g. from print("[aborted]")) is preserved and exits with code 0.
 * - Macro aborts or throws an uncaught error -> logs [oracle-macro-aborted] to stderr and exits with code 1.
 * - Non-eval invocations (e.g. doctor, file opens) delegate directly to ImageJ.main.
 */
public class OracleRunner {
    public static final String ABORT_MARKER = "[oracle-macro-aborted]";

    public static void main(String[] args) {
        int evalIdx = -1;
        for (int i = 0; i < args.length; i++) {
            if ("-eval".equals(args[i]) && i + 1 < args.length) {
                evalIdx = i;
                break;
            }
        }

        if (evalIdx >= 0) {
            String macro = args[evalIdx + 1];
            Interpreter.batchMode = true;
            try {
                Interpreter interpreter = new Interpreter();
                String rtn = interpreter.run(macro, "");
                if (interpreter.wasError() || "[aborted]".equals(rtn)) {
                    System.err.println(ABORT_MARKER);
                    System.exit(1);
                }
                if (rtn != null) {
                    System.out.print(rtn);
                }
                System.exit(0);
            } catch (Throwable t) {
                System.err.println(ABORT_MARKER + " Exception: " + t.getMessage());
                t.printStackTrace(System.err);
                System.exit(1);
            }
        } else {
            ImageJ.main(args);
        }
    }
}
