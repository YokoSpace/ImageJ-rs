package fixtures;
import ij.ImagePlus;
import ij.gui.GenericDialog;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;
public class UnsupportedDialogFilter implements PlugInFilter {
    public int setup(String arg, ImagePlus imp) {
        GenericDialog gd = new GenericDialog("Slider");
        gd.addSlider("Amount", 0, 10, 1);
        gd.showDialog();
        return DOES_8G;
    }
    public void run(ImageProcessor ip) { ip.invert(); }
}
