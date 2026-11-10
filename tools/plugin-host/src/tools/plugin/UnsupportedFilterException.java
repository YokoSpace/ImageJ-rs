package tools.plugin;

/**
 * Signals that the narrow PlugInFilter host cannot faithfully execute a
 * request. It is deliberately distinct from a plugin exception: the request
 * itself is outside the supported subset, so no output must be produced.
 */
final class UnsupportedFilterException extends RuntimeException {

    private final int flags;

    UnsupportedFilterException(String message) {
        this(message, 0);
    }

    UnsupportedFilterException(String message, int flags) {
        super(message);
        this.flags = flags;
    }

    int flags() {
        return flags;
    }
}
