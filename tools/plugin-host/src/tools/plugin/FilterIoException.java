package tools.plugin;

/**
 * Signals that the host could not read its input or publish its output.
 *
 * This is deliberately distinct from an unsupported request: the request may
 * be valid, but the transfer failed. The host reports it as `io_error` so the
 * Rust bridge surfaces the real I/O failure instead of claiming the filter
 * subset is unsupported.
 */
final class FilterIoException extends RuntimeException {

    FilterIoException(String message) {
        super(message);
    }

    FilterIoException(String message, Throwable cause) {
        super(message, cause);
    }
}
