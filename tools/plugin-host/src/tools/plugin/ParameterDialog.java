package tools.plugin;

import ij.Macro;
import java.util.*;

/** Single-dialog, headless subset of GenericDialog's macro contract. */
public final class ParameterDialog {
    static String options;
    static boolean created;
    private final String title;
    private final List<Field> fields = new ArrayList<>();
    private final Map<String, Integer> indexes = new HashMap<>();
    private boolean invalidNumber;
    private boolean shown;

    static final class Required extends Error {
        final ParameterDialog dialog;
        Required(ParameterDialog dialog) { this.dialog = dialog; }
    }

    private static final class Field {
        String type, key, label, value, units;
        int digits;
        String[] choices;
    }

    public ParameterDialog(String title) {
        if (created) throw new UnsupportedFilterException("Only one GenericDialog is supported per filter run");
        created = true;
        this.title = title;
    }

    public ParameterDialog(String title, java.awt.Frame parent) { this(title); }

    private Field add(String type, String label, String value) {
        if (shown) throw new UnsupportedFilterException("Fields added after showDialog are unsupported");
        Field f = new Field();
        f.type = type; f.label = label; f.value = value;
        String base = Macro.trimKey(label.trim());
        if (base.isEmpty()) throw new UnsupportedFilterException("Empty GenericDialog field key");
        String key = base;
        int suffix = 0;
        // GenericDialog.saveLabel, lines 233-242.
        while (hasKey(key)) key = base + "_" + suffix++;
        f.key = key;
        fields.add(f);
        return f;
    }

    private boolean hasKey(String key) {
        for (Field field : fields) if (field.key.equals(key)) return true;
        return false;
    }

    public void addNumericField(String label, double value) {
        addNumericField(label, value, (int)value == value ? 0 : 3);
    }
    public void addNumericField(String label, double value, int digits) {
        addNumericField(label, value, digits, 6, null);
    }
    public void addNumericField(String label, double value, int digits, int columns, String units) {
        Field f = add("numeric", label, Double.toString(value));
        f.digits = digits; f.units = units;
    }
    public void addStringField(String label, String value) { addStringField(label, value, 8); }
    public void addStringField(String label, String value, int columns) { add("string", label, value == null ? "" : value); }
    public void addCheckbox(String label, boolean value) { add("checkbox", label, Boolean.toString(value)); }
    public void addChoice(String label, String[] items, String value) {
        if (items.length == 0) throw new UnsupportedFilterException("Empty GenericDialog choice");
        if (!Arrays.asList(items).contains(value)) value = items[0];
        Field f = add("choice", label, value); f.choices = items.clone();
    }
    public void showDialog() {
        if (shown) throw new UnsupportedFilterException("Repeated showDialog is unsupported");
        shown = true;
        if (options == null) throw new Required(this);
    }
    public boolean wasCanceled() { return false; }
    public boolean wasOKed() { return shown; }
    public String getTitle() { return title; }
    private Field next(String type) {
        int index = indexes.getOrDefault(type, 0);
        indexes.put(type, index + 1);
        for (Field f : fields) if (f.type.equals(type) && index-- == 0) return f;
        throw new UnsupportedFilterException("GenericDialog getter has no matching " + type + " field");
    }
    private String value(Field f) { return Macro.getValue(options, f.key, f.value); }
    public double getNextNumber() {
        Field f = next("numeric");
        String text = value(f);
        if (text.equals(f.value)) return Double.parseDouble(f.value);
        try {
            if (text.startsWith("0x")) return Integer.parseInt(text.substring(2), 16);
            return Double.parseDouble(text);
        } catch (NumberFormatException e) { invalidNumber = true; return Double.NaN; }
    }
    public String getNextString() { return value(next("string")); }
    public String getNextChoice() { return value(next("choice")); }
    public int getNextChoiceIndex() {
        Field f = next("choice");
        int index = Arrays.asList(f.choices).indexOf(value(f));
        if (index < 0) throw new UnsupportedFilterException("Invalid choice for " + f.key);
        return index;
    }
    public boolean getNextBoolean() {
        String key = next("checkbox").key;
        // GenericDialog.getNextBoolean/isMatch, lines 1341-1383: only bare
        // tokens outside bracket literals enable a checkbox in macro mode.
        String text = options + " ";
        boolean literal = false;
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c == '[') literal = true;
            else if (c == ']') literal = false;
            if (!literal && (i == 0 || text.charAt(i - 1) == ' ')
                    && text.startsWith(key + " ", i)) return true;
        }
        return false;
    }
    public boolean invalidNumber() { boolean result = invalidNumber; invalidNumber = false; return result; }
    public String getErrorMessage() { return invalidNumber ? "Invalid numeric parameter" : null; }

    static String quote(String value) {
        if (value == null) return "null";
        StringBuilder b = new StringBuilder("\"");
        for (char c : value.toCharArray()) {
            if (c == '\\' || c == '"') b.append('\\').append(c);
            else if (c < 32) b.append(String.format("\\u%04x", (int)c));
            else b.append(c);
        }
        return b.append('"').toString();
    }
    String json() {
        StringBuilder b = new StringBuilder("{\"title\":" + quote(title) + ",\"fields\":[");
        for (int i = 0; i < fields.size(); i++) {
            if (i != 0) b.append(',');
            Field f = fields.get(i);
            b.append("{\"type\":").append(quote(f.type))
                .append(",\"key\":").append(quote(f.key))
                .append(",\"label\":").append(quote(f.label))
                .append(",\"default\":").append(quote(f.value));
            if (f.type.equals("numeric")) b.append(",\"digits\":").append(f.digits)
                .append(",\"units\":").append(quote(f.units));
            if (f.choices != null) {
                b.append(",\"choices\":[");
                for (int n = 0; n < f.choices.length; n++) {
                    if (n > 0) b.append(',');
                    b.append(quote(f.choices[n]));
                }
                b.append(']');
            }
            b.append('}');
        }
        return b.append("]}").toString();
    }
}
