package fixtures;

import ij.ImagePlus;
import ij.gui.GenericDialog;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;

public class ParameterFilter implements PlugInFilter {
    private double amount;
    private boolean enabled;
    private String text;
    private int mode;

    public int setup(String arg, ImagePlus imp) {
        GenericDialog gd = new GenericDialog("Fixture parameters");
        gd.addNumericField("Amount:", 2.5, 2, 8, "levels");
        gd.addStringField("Text_value:", "default text", 20);
        gd.addChoice("Mode:", new String[]{"Add", "Subtract value"}, "Add");
        gd.addCheckbox("Enabled", true);
        gd.showDialog();
        if (gd.wasCanceled()) return DONE;
        amount = gd.getNextNumber();
        text = gd.getNextString();
        mode = gd.getNextChoiceIndex();
        enabled = gd.getNextBoolean();
        System.out.println("PARAMETERS=" + amount + ":" + text + ":" + mode + ":" + enabled);
        return DOES_8G;
    }

    public void run(ImageProcessor ip) {
        if (enabled) ip.add((mode == 0 ? amount : -amount) + text.length());
    }
}
