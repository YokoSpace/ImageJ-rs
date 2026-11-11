package fixtures;
import ij.ImagePlus;
import ij.gui.GenericDialog;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;
public class MultipleDialogsFilter implements PlugInFilter {
    public int setup(String arg, ImagePlus imp) {
        GenericDialog first = new GenericDialog("First");
        first.addCheckbox("Enabled", true);
        first.showDialog();
        GenericDialog second = new GenericDialog("Second");
        second.addNumericField("Amount", 1, 0);
        second.showDialog();
        return DOES_8G;
    }
    public void run(ImageProcessor ip) { ip.invert(); }
}
