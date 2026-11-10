# ImageJ 1.54u command ledger

Generated from `original_imagej/ij/Menus.java` and `original_imagej/IJ_Props.txt` by `tools/ledger/sync_ledger.py`. Do not edit by hand.

- Total registered menu commands: **414**
- Rust Vertical Slice 1 target (`RustTier1`): **17**
- Delegated to Java compatibility session (`JavaBridge`): **397**

| Key | Menu section | Command label | Shortcut | Java class | Arg | Target tier |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `menus_01` | Menus.java (file) | `Open...` | `O` | `ij.plugin.Commands` | `open` | `RustTier1` |
| `menus_02` | Menus.java (file) | `Open Next` | `Shift+O` | `ij.plugin.NextImageOpener` | `` | `JavaBridge` |
| `menus_03` | Menus.java (openSamples) | `Cache Sample Images ` | `` | `ij.plugin.URLOpener` | `cache` | `JavaBridge` |
| `menus_04` | Menus.java (showFolderMenu) | `Image` | `` | `ij.plugin.SimpleCommands` | `showdirImage` | `JavaBridge` |
| `menus_05` | Menus.java (showFolderMenu) | `Plugins` | `` | `ij.plugin.SimpleCommands` | `showdirPlugins` | `JavaBridge` |
| `menus_06` | Menus.java (showFolderMenu) | `Macros` | `` | `ij.plugin.SimpleCommands` | `showdirMacros` | `JavaBridge` |
| `menus_07` | Menus.java (showFolderMenu) | `LUTs` | `` | `ij.plugin.SimpleCommands` | `showdirLuts` | `JavaBridge` |
| `menus_08` | Menus.java (showFolderMenu) | `ImageJ` | `` | `ij.plugin.SimpleCommands` | `showdirImageJ` | `JavaBridge` |
| `menus_09` | Menus.java (showFolderMenu) | `temp` | `` | `ij.plugin.SimpleCommands` | `showdirTemp` | `JavaBridge` |
| `menus_10` | Menus.java (showFolderMenu) | `Home` | `` | `ij.plugin.SimpleCommands` | `showdirHome` | `JavaBridge` |
| `menus_11` | Menus.java (file) | `Close` | `W` | `ij.plugin.Commands` | `close` | `JavaBridge` |
| `menus_12` | Menus.java (file) | `Close All` | `Shift+W` | `ij.plugin.Commands` | `close-all` | `JavaBridge` |
| `menus_13` | Menus.java (file) | `Save` | `S` | `ij.plugin.Commands` | `save` | `RustTier1` |
| `menus_14` | Menus.java (file) | `Revert` | `Shift+R` | `ij.plugin.Commands` | `revert` | `JavaBridge` |
| `menus_15` | Menus.java (file) | `Page Setup...` | `` | `ij.plugin.filter.Printer` | `setup` | `JavaBridge` |
| `menus_16` | Menus.java (file) | `Print...` | `P` | `ij.plugin.filter.Printer` | `print` | `JavaBridge` |
| `menus_17` | Menus.java (edit) | `Undo` | `Z` | `ij.plugin.Commands` | `undo` | `RustTier1` |
| `menus_18` | Menus.java (edit) | `Cut` | `X` | `ij.plugin.Clipboard` | `cut` | `JavaBridge` |
| `menus_19` | Menus.java (edit) | `Copy` | `C` | `ij.plugin.Clipboard` | `copy` | `JavaBridge` |
| `menus_20` | Menus.java (edit) | `Copy to System` | `` | `ij.plugin.Clipboard` | `scopy` | `JavaBridge` |
| `menus_21` | Menus.java (edit) | `Paste` | `V` | `ij.plugin.Clipboard` | `paste` | `JavaBridge` |
| `menus_22` | Menus.java (edit) | `Paste Control...` | `` | `ij.plugin.frame.PasteController` | `` | `JavaBridge` |
| `menus_23` | Menus.java (edit) | `Clear` | `` | `ij.plugin.filter.Filler` | `clear` | `JavaBridge` |
| `menus_24` | Menus.java (edit) | `Clear Outside` | `` | `ij.plugin.filter.Filler` | `outside` | `JavaBridge` |
| `menus_25` | Menus.java (edit) | `Fill` | `F` | `ij.plugin.filter.Filler` | `fill` | `JavaBridge` |
| `menus_26` | Menus.java (edit) | `Draw` | `D` | `ij.plugin.filter.Filler` | `draw` | `JavaBridge` |
| `menus_27` | Menus.java (edit) | `Invert` | `Shift+I` | `ij.plugin.filter.Filters` | `invert` | `RustTier1` |
| `menus_28` | Menus.java (imageType) | `8-bit` | `` | `ij.plugin.Converter` | `8-bit` | `RustTier1` |
| `menus_29` | Menus.java (imageType) | `16-bit` | `` | `ij.plugin.Converter` | `16-bit` | `RustTier1` |
| `menus_30` | Menus.java (imageType) | `32-bit` | `` | `ij.plugin.Converter` | `32-bit` | `RustTier1` |
| `menus_31` | Menus.java (imageType) | `8-bit Color` | `` | `ij.plugin.Converter` | `8-bit Color` | `JavaBridge` |
| `menus_32` | Menus.java (imageType) | `RGB Color` | `` | `ij.plugin.Converter` | `RGB Color` | `RustTier1` |
| `menus_33` | Menus.java (imageType) | `RGB Stack` | `` | `ij.plugin.Converter` | `RGB Stack` | `JavaBridge` |
| `menus_34` | Menus.java (imageType) | `HSB Stack` | `` | `ij.plugin.Converter` | `HSB Stack` | `JavaBridge` |
| `menus_35` | Menus.java (imageType) | `HSB (32-bit)` | `` | `ij.plugin.Converter` | `HSB (32-bit)` | `JavaBridge` |
| `menus_36` | Menus.java (imageType) | `Lab Stack` | `` | `ij.plugin.Converter` | `Lab Stack` | `JavaBridge` |
| `menus_37` | Menus.java (image) | `Show Info...` | `I` | `ij.plugin.ImageInfo` | `` | `JavaBridge` |
| `menus_38` | Menus.java (image) | `Properties...` | `Shift+P` | `ij.plugin.filter.ImageProperties` | `` | `JavaBridge` |
| `menus_39` | Menus.java (image) | `Crop` | `Shift+X` | `ij.plugin.Resizer` | `crop` | `JavaBridge` |
| `menus_40` | Menus.java (image) | `Duplicate...` | `Shift+D` | `ij.plugin.Duplicator` | `` | `RustTier1` |
| `menus_41` | Menus.java (image) | `Rename...` | `` | `ij.plugin.SimpleCommands` | `rename` | `JavaBridge` |
| `menus_42` | Menus.java (image) | `Scale...` | `E` | `ij.plugin.Scaler` | `` | `JavaBridge` |
| `menus_43` | Menus.java (process) | `Smooth` | `Shift+S` | `ij.plugin.filter.Filters` | `smooth` | `JavaBridge` |
| `menus_44` | Menus.java (process) | `Sharpen` | `` | `ij.plugin.filter.Filters` | `sharpen` | `JavaBridge` |
| `menus_45` | Menus.java (process) | `Find Edges` | `` | `ij.plugin.filter.Filters` | `edge` | `JavaBridge` |
| `menus_46` | Menus.java (process) | `Find Maxima...` | `` | `ij.plugin.filter.MaximumFinder` | `` | `JavaBridge` |
| `menus_47` | Menus.java (process) | `Enhance Contrast...` | `` | `ij.plugin.ContrastEnhancer` | `` | `JavaBridge` |
| `menus_48` | Menus.java (process) | `Image Calculator...` | `` | `ij.plugin.ImageCalculator` | `` | `JavaBridge` |
| `menus_49` | Menus.java (process) | `Subtract Background...` | `` | `ij.plugin.filter.BackgroundSubtracter` | `` | `JavaBridge` |
| `menus_50` | Menus.java (analyzeMenu) | `Measure` | `M` | `ij.plugin.filter.Analyzer` | `` | `RustTier1` |
| `menus_51` | Menus.java (analyzeMenu) | `Analyze Particles...` | `` | `ij.plugin.filter.ParticleAnalyzer` | `` | `JavaBridge` |
| `menus_52` | Menus.java (analyzeMenu) | `Summarize` | `` | `ij.plugin.filter.Analyzer` | `sum` | `JavaBridge` |
| `menus_53` | Menus.java (analyzeMenu) | `Distribution...` | `` | `ij.plugin.Distribution` | `` | `JavaBridge` |
| `menus_54` | Menus.java (analyzeMenu) | `Label` | `` | `ij.plugin.filter.Filler` | `label` | `JavaBridge` |
| `menus_55` | Menus.java (analyzeMenu) | `Clear Results` | `` | `ij.plugin.filter.Analyzer` | `clear` | `RustTier1` |
| `menus_56` | Menus.java (analyzeMenu) | `Set Measurements...` | `` | `ij.plugin.filter.Analyzer` | `set` | `RustTier1` |
| `menus_57` | Menus.java (analyzeMenu) | `Set Scale...` | `` | `ij.plugin.filter.ScaleDialog` | `` | `JavaBridge` |
| `menus_58` | Menus.java (analyzeMenu) | `Calibrate...` | `` | `ij.plugin.filter.Calibrator` | `` | `JavaBridge` |
| `menus_59` | Menus.java (analyzeMenu) | `Histogram` | `` | `ij.plugin.Histogram` | `` | `RustTier1` |
| `menus_60` | Menus.java (analyzeMenu) | `Plot Profile` | `K` | `ij.plugin.Profiler` | `plot` | `JavaBridge` |
| `menus_61` | Menus.java (analyzeMenu) | `Surface Plot...` | `` | `ij.plugin.SurfacePlotter` | `` | `JavaBridge` |
| `menus_62` | Menus.java (window) | `Show All` | `CLOSE_BRACKET` | `ij.plugin.WindowOrganizer` | `show` | `JavaBridge` |
| `menus_63` | Menus.java (window) | `Put Behind [tab]` | `` | `ij.plugin.Commands` | `tab` | `JavaBridge` |
| `menus_64` | Menus.java (window) | `Cascade` | `` | `ij.plugin.WindowOrganizer` | `cascade` | `JavaBridge` |
| `menus_65` | Menus.java (window) | `Tile` | `` | `ij.plugin.WindowOrganizer` | `tile` | `JavaBridge` |
| `menus_66` | Menus.java (help) | `ImageJ Website...` | `` | `ij.plugin.BrowserLauncher` | `` | `JavaBridge` |
| `menus_67` | Menus.java (help) | `Macro Functions...` | `` | `ij.plugin.BrowserLauncher` | `https://wsr.imagej.net/developer/macro/functions.html` | `JavaBridge` |
| `menus_68` | Menus.java (examplesMenu) | `Open as Panel` | `` | `ij.plugin.SimpleCommands` | `opencp` | `JavaBridge` |
| `menus_69` | Menus.java (help) | `Update ImageJ...` | `` | `ij.plugin.ImageJ_Updater` | `` | `JavaBridge` |
| `menus_70` | Menus.java (help) | `Release Notes...` | `` | `ij.plugin.BrowserLauncher` | `https://wsr.imagej.net/notes.html` | `JavaBridge` |
| `menus_71` | Menus.java (help) | `Refresh Menus` | `` | `ij.plugin.ImageJ_Updater` | `menus` | `JavaBridge` |
| `menus_72` | Menus.java (help) | `About ImageJ...` | `` | `ij.plugin.AboutBox` | `` | `JavaBridge` |
| `menus_73` | Menus.java (file) | `Quit` | `` | `ij.plugin.Commands` | `quit` | `JavaBridge` |
| `menus_74` | Menus.java (pluginsMenu) | `Why are Plugins Missing?` | `` | `ij.plugin.SimpleCommands` | `missing` | `JavaBridge` |
| `new01` | File/New | `Image...` | `n` | `ij.plugin.Commands` | `new` | `JavaBridge` |
| `new02` | File/New | `Hyperstack...` | `` | `ij.plugin.HyperStackMaker` | `` | `JavaBridge` |
| `new03` | File/New | `Text Window` | `N` | `ij.plugin.NewPlugin` | `text` | `JavaBridge` |
| `new04` | File/New | `Internal Clipboard` | `` | `ij.plugin.Clipboard` | `show` | `JavaBridge` |
| `new05` | File/New | `System Clipboard` | `V` | `ij.plugin.Clipboard` | `showsys` | `JavaBridge` |
| `import01` | File/Import | `Image Sequence...` | `` | `ij.plugin.FolderOpener` | `` | `JavaBridge` |
| `import02` | File/Import | `Raw...` | `` | `ij.plugin.Raw` | `` | `JavaBridge` |
| `import03` | File/Import | `LUT... ` | `` | `ij.plugin.LutLoader` | `` | `JavaBridge` |
| `import04` | File/Import | `Text Image... ` | `` | `ij.plugin.TextReader` | `` | `JavaBridge` |
| `import05` | File/Import | `Text File... ` | `` | `ij.plugin.TextFileReader` | `` | `JavaBridge` |
| `import06` | File/Import | `Results... ` | `` | `ij.plugin.SimpleCommands` | `import` | `RustTier1` |
| `import07` | File/Import | `Table... ` | `` | `ij.plugin.SimpleCommands` | `table` | `JavaBridge` |
| `import08` | File/Import | `URL...` | `` | `ij.plugin.URLOpener` | `` | `JavaBridge` |
| `import09` | File/Import | `Stack From List...` | `` | `ij.plugin.ListVirtualStack` | `` | `JavaBridge` |
| `import10` | File/Import | `TIFF Virtual Stack...` | `` | `ij.plugin.FileInfoVirtualStack` | `` | `JavaBridge` |
| `import11` | File/Import | `AVI...` | `` | `ij.plugin.AVI_Reader` | `` | `JavaBridge` |
| `import12` | File/Import | `XY Coordinates... ` | `` | `ij.plugin.XY_Reader` | `` | `JavaBridge` |
| `save01` | File/Save A | `Tiff...` | `` | `ij.plugin.filter.Writer` | `tiff` | `RustTier1` |
| `save02` | File/Save A | `Gif...` | `` | `ij.plugin.filter.Writer` | `gif` | `JavaBridge` |
| `save03` | File/Save A | `Jpeg...` | `` | `ij.plugin.filter.Writer` | `jpeg` | `JavaBridge` |
| `save04` | File/Save A | `Text Image...` | `` | `ij.plugin.filter.Writer` | `text` | `JavaBridge` |
| `save05` | File/Save A | `ZIP...` | `` | `ij.plugin.filter.Writer` | `zip` | `JavaBridge` |
| `save06` | File/Save A | `Raw Data...` | `` | `ij.plugin.filter.Writer` | `raw` | `JavaBridge` |
| `save07` | File/Save A | `Image Sequence... ` | `` | `ij.plugin.StackWriter` | `` | `JavaBridge` |
| `save08` | File/Save A | `AVI... ` | `` | `ij.plugin.filter.AVI_Writer` | `` | `JavaBridge` |
| `save09` | File/Save A | `BMP...` | `` | `ij.plugin.filter.Writer` | `bmp` | `JavaBridge` |
| `save10` | File/Save A | `PNG...` | `` | `ij.plugin.filter.Writer` | `png` | `JavaBridge` |
| `save11` | File/Save A | `PGM...` | `` | `ij.plugin.filter.Writer` | `pgm` | `JavaBridge` |
| `save12` | File/Save A | `FITS...` | `` | `ij.plugin.filter.Writer` | `fits` | `JavaBridge` |
| `save13` | File/Save A | `LUT...` | `` | `ij.plugin.filter.Writer` | `lut` | `JavaBridge` |
| `save14` | File/Save A | `Selection...` | `` | `ij.plugin.filter.RoiWriter` | `` | `JavaBridge` |
| `save15` | File/Save A | `XY Coordinates...` | `` | `ij.plugin.filter.XYWriter` | `` | `JavaBridge` |
| `save16` | File/Save A | `Results...` | `` | `ij.plugin.MeasurementsWriter` | `` | `RustTier1` |
| `save17` | File/Save A | `Text...` | `` | `ij.plugin.TextWriter` | `` | `JavaBridge` |
| `selection01` | Edit/Selectio | `Select All` | `a` | `ij.plugin.Selection` | `all` | `JavaBridge` |
| `selection02` | Edit/Selectio | `Select None` | `A` | `ij.plugin.Selection` | `none` | `JavaBridge` |
| `selection03` | Edit/Selectio | `Restore Selection` | `E` | `ij.plugin.Selection` | `restore` | `JavaBridge` |
| `selection04` | Edit/Selectio | `Fit Spline` | `` | `ij.plugin.Selection` | `spline` | `JavaBridge` |
| `selection05` | Edit/Selectio | `Fit Circle` | `` | `ij.plugin.Selection` | `circle` | `JavaBridge` |
| `selection06` | Edit/Selectio | `Fit Ellipse` | `` | `ij.plugin.Selection` | `ellipse` | `JavaBridge` |
| `selection07` | Edit/Selectio | `Fit Rectangle` | `` | `ij.plugin.Selection` | `rect` | `JavaBridge` |
| `selection08` | Edit/Selectio | `Interpolate` | `` | `ij.plugin.Selection` | `interpolate` | `JavaBridge` |
| `selection09` | Edit/Selectio | `Convex Hull` | `` | `ij.plugin.Selection` | `hull` | `JavaBridge` |
| `selection10` | Edit/Selectio | `Make Inverse` | `` | `ij.plugin.Selection` | `inverse` | `JavaBridge` |
| `selection11` | Edit/Selectio | `Create Selection` | `` | `ij.plugin.Selection` | `from` | `JavaBridge` |
| `selection12` | Edit/Selectio | `Create Mask` | `` | `ij.plugin.Selection` | `mask` | `JavaBridge` |
| `selection14` | Edit/Selectio | `Properties... ` | `y` | `ij.plugin.Selection` | `properties` | `JavaBridge` |
| `selection15` | Edit/Selectio | `Scale... ` | `` | `ij.plugin.RoiScaler` | `` | `JavaBridge` |
| `selection16` | Edit/Selectio | `Rotate...` | `` | `ij.plugin.Selection` | `rotate` | `JavaBridge` |
| `selection17` | Edit/Selectio | `Translate... ` | `` | `ij.plugin.Selection` | `translate` | `JavaBridge` |
| `selection18` | Edit/Selectio | `Enlarge...` | `` | `ij.plugin.RoiEnlarger` | `` | `JavaBridge` |
| `selection19` | Edit/Selectio | `Make Band...` | `` | `ij.plugin.Selection` | `band` | `JavaBridge` |
| `selection20` | Edit/Selectio | `Specify...` | `` | `ij.plugin.SpecifyROI` | `` | `JavaBridge` |
| `selection21` | Edit/Selectio | `Straighten...` | `` | `ij.plugin.Straightener` | `` | `JavaBridge` |
| `selection22` | Edit/Selectio | `To Bounding Box` | `` | `ij.plugin.Selection` | `tobox` | `JavaBridge` |
| `selection23` | Edit/Selectio | `Line to Area` | `` | `ij.plugin.Selection` | `toarea` | `JavaBridge` |
| `selection24` | Edit/Selectio | `Area to Line` | `` | `ij.plugin.Selection` | `toline` | `JavaBridge` |
| `selection25` | Edit/Selectio | `Image to Selection...` | `` | `ij.plugin.OverlayCommands` | `image-roi` | `JavaBridge` |
| `selection26` | Edit/Selectio | `Add to Manager` | `t` | `ij.plugin.Selection` | `add` | `JavaBridge` |
| `options01` | Edit/Optio | `Appearance...` | `` | `ij.plugin.AppearanceOptions` | `` | `JavaBridge` |
| `options02` | Edit/Optio | `Arrow Tool...` | `` | `ij.plugin.ArrowToolOptions` | `` | `JavaBridge` |
| `options03` | Edit/Optio | `Colors...` | `` | `ij.plugin.Colors` | `` | `JavaBridge` |
| `options04` | Edit/Optio | `Compiler...` | `` | `ij.plugin.Compiler` | `options` | `JavaBridge` |
| `options05` | Edit/Optio | `Conversions...` | `` | `ij.plugin.Options` | `conv` | `JavaBridge` |
| `options06` | Edit/Optio | `DICOM...` | `` | `ij.plugin.Options` | `dicom` | `JavaBridge` |
| `options07` | Edit/Optio | `Fonts...` | `` | `ij.plugin.SimpleCommands` | `fonts` | `JavaBridge` |
| `options08` | Edit/Optio | `Input/Output...` | `` | `ij.plugin.Options` | `io` | `JavaBridge` |
| `options09` | Edit/Optio | `Line Width...` | `` | `ij.plugin.Options` | `line` | `JavaBridge` |
| `options10` | Edit/Optio | `Memory & Threads...` | `` | `ij.plugin.Memory` | `` | `JavaBridge` |
| `options11` | Edit/Optio | `Misc...` | `` | `ij.plugin.Options` | `misc` | `JavaBridge` |
| `options12` | Edit/Optio | `Plots...` | `` | `ij.plugin.Profiler` | `set` | `JavaBridge` |
| `options13` | Edit/Optio | `Point Tool...` | `` | `ij.plugin.PointToolOptions` | `` | `JavaBridge` |
| `options14` | Edit/Optio | `Proxy Settings...` | `` | `ij.plugin.ProxySettings` | `` | `JavaBridge` |
| `options15` | Edit/Optio | `Roi Defaults...` | `` | `ij.gui.RoiDefaultsDialog` | `` | `JavaBridge` |
| `options16` | Edit/Optio | `Rounded Rect Tool...` | `` | `ij.plugin.RectToolOptions` | `` | `JavaBridge` |
| `options17` | Edit/Optio | `Startup...` | `` | `ij.plugin.Startup` | `` | `JavaBridge` |
| `options18` | Edit/Optio | `Wand Tool...` | `` | `ij.plugin.WandToolOptions` | `` | `JavaBridge` |
| `options20` | Edit/Optio | `Fresh Start` | `` | `ij.plugin.Options` | `fresh-start` | `JavaBridge` |
| `options21` | Edit/Optio | `Reset... ` | `` | `ij.plugin.Options` | `reset` | `JavaBridge` |
| `adjust01` | Image/Adjust | `Brightness/Contrast...` | `C` | `ij.plugin.frame.ContrastAdjuster` | `` | `RustTier1` |
| `adjust02` | Image/Adjust | `Window/Level...` | `` | `ij.plugin.frame.ContrastAdjuster` | `wl` | `JavaBridge` |
| `adjust03` | Image/Adjust | `Color Balance...` | `` | `ij.plugin.frame.ContrastAdjuster` | `balance` | `JavaBridge` |
| `adjust04` | Image/Adjust | `Threshold...` | `T` | `ij.plugin.frame.ThresholdAdjuster` | `` | `JavaBridge` |
| `adjust05` | Image/Adjust | `Color Threshold...` | `` | `ij.plugin.frame.ColorThresholder` | `` | `JavaBridge` |
| `adjust06` | Image/Adjust | `Size...` | `` | `ij.plugin.Resizer` | `` | `JavaBridge` |
| `adjust07` | Image/Adjust | `Canvas Size...` | `` | `ij.plugin.CanvasResizer` | `` | `JavaBridge` |
| `adjust08` | Image/Adjust | `Line Width... ` | `` | `ij.plugin.frame.LineWidthAdjuster` | `` | `JavaBridge` |
| `adjust09` | Image/Adjust | `Coordinates...` | `` | `ij.plugin.Coordinates` | `` | `JavaBridge` |
| `color01` | Image/Color | `Split Channels` | `` | `ij.plugin.ChannelSplitter` | `` | `JavaBridge` |
| `color02` | Image/Color | `Merge Channels...` | `` | `ij.plugin.RGBStackMerge` | `` | `JavaBridge` |
| `color03` | Image/Color | `Arrange Channels...` | `` | `ij.plugin.ChannelArranger` | `` | `JavaBridge` |
| `color04` | Image/Color | `Invert LUTs` | `` | `ij.plugin.SimpleCommands` | `invert` | `JavaBridge` |
| `color05` | Image/Color | `Channels Tool...` | `Z` | `ij.plugin.frame.Channels` | `` | `JavaBridge` |
| `color07` | Image/Color | `Stack to RGB` | `` | `ij.plugin.RGBStackConverter` | `` | `JavaBridge` |
| `color08` | Image/Color | `Make Composite` | `` | `ij.plugin.CompositeConverter` | `` | `JavaBridge` |
| `color09` | Image/Color | `Show LUT` | `` | `ij.plugin.filter.LutViewer` | `` | `JavaBridge` |
| `color10` | Image/Color | `Display LUTs` | `` | `ij.plugin.SimpleCommands` | `display` | `JavaBridge` |
| `color11` | Image/Color | `Edit LUT...` | `` | `ij.plugin.LUT_Editor` | `` | `JavaBridge` |
| `color12` | Image/Color | `Color Picker...` | `K` | `ij.plugin.frame.ColorPicker` | `` | `JavaBridge` |
| `stacks01` | Image/Stack | `Add Slice` | `` | `ij.plugin.StackEditor` | `add` | `JavaBridge` |
| `stacks02` | Image/Stack | `Delete Slice` | `` | `ij.plugin.StackEditor` | `delete` | `JavaBridge` |
| `stacks03` | Image/Stack | `Next Slice ` | `>` | `ij.plugin.Animator` | `next` | `JavaBridge` |
| `stacks04` | Image/Stack | `Previous Slice ` | `<` | `ij.plugin.Animator` | `previous` | `JavaBridge` |
| `stacks05` | Image/Stack | `Set Slice...` | `` | `ij.plugin.Animator` | `set` | `JavaBridge` |
| `stacks07` | Image/Stack | `Images to Stack` | `` | `ij.plugin.ImagesToStack` | `` | `JavaBridge` |
| `stacks08` | Image/Stack | `Stack to Images` | `` | `ij.plugin.StackEditor` | `toimages` | `JavaBridge` |
| `stacks09` | Image/Stack | `Make Montage...` | `` | `ij.plugin.MontageMaker` | `` | `JavaBridge` |
| `stacks10` | Image/Stack | `Reslice [/]...` | `` | `ij.plugin.Slicer` | `` | `JavaBridge` |
| `stacks11` | Image/Stack | `Orthogonal Views` | `H` | `ij.plugin.Orthogonal_Views` | `` | `JavaBridge` |
| `stacks12` | Image/Stack | `Z Project...` | `` | `ij.plugin.ZProjector` | `` | `JavaBridge` |
| `stacks13` | Image/Stack | `3D Project...` | `` | `ij.plugin.Projector` | `` | `JavaBridge` |
| `stacks14` | Image/Stack | `Plot Z-axis Profile` | `` | `ij.plugin.ZAxisProfiler` | `` | `JavaBridge` |
| `stacks15` | Image/Stack | `Measure Stack...` | `` | `ij.plugin.SimpleCommands` | `measure` | `JavaBridge` |
| `stacks16` | Image/Stack | `Label...` | `` | `ij.plugin.filter.StackLabeler` | `` | `JavaBridge` |
| `stacks17` | Image/Stack | `Statistics` | `` | `ij.plugin.Stack_Statistics` | `` | `JavaBridge` |
| `animation_01` | Image/Stacks/Animatio | `Start Animation ` | `\\` | `ij.plugin.Animator` | `start` | `JavaBridge` |
| `animation_02` | Image/Stacks/Animatio | `Stop Animation` | `` | `ij.plugin.Animator` | `stop` | `JavaBridge` |
| `animation_03` | Image/Stacks/Animatio | `Animation Options...` | `` | `ij.plugin.Animator` | `options` | `JavaBridge` |
| `tools_01` | Image/Stacks/Tool | `Combine...` | `` | `ij.plugin.StackCombiner` | `` | `JavaBridge` |
| `tools_02` | Image/Stacks/Tool | `Concatenate...` | `` | `ij.plugin.Concatenator` | `` | `JavaBridge` |
| `tools_03` | Image/Stacks/Tool | `Grouped Z Project...` | `` | `ij.plugin.GroupedZProjector` | `` | `JavaBridge` |
| `tools_04` | Image/Stacks/Tool | `Insert...` | `` | `ij.plugin.StackInserter` | `` | `JavaBridge` |
| `tools_05` | Image/Stacks/Tool | `Magic Montage Tools` | `` | `ij.plugin.SimpleCommands` | `magic` | `JavaBridge` |
| `tools_06` | Image/Stacks/Tool | `Make Substack...` | `` | `ij.plugin.SubstackMaker` | `` | `JavaBridge` |
| `tools_07` | Image/Stacks/Tool | `Montage to Stack...` | `` | `ij.plugin.StackMaker` | `` | `JavaBridge` |
| `tools_08` | Image/Stacks/Tool | `Plot XY Profile` | `` | `ij.plugin.StackPlotter` | `` | `JavaBridge` |
| `tools_09` | Image/Stacks/Tool | `Reduce...` | `` | `ij.plugin.StackReducer` | `` | `JavaBridge` |
| `tools_10` | Image/Stacks/Tool | `Remove Slice Labels` | `` | `ij.plugin.SimpleCommands` | `remove` | `JavaBridge` |
| `tools_11` | Image/Stacks/Tool | `Reverse` | `` | `ij.plugin.StackReverser` | `` | `JavaBridge` |
| `tools_12` | Image/Stacks/Tool | `Set Label...` | `` | `ij.plugin.SimpleCommands` | `set` | `JavaBridge` |
| `hyperstacks01` | Image/Hyperstack | `New Hyperstack...` | `` | `ij.plugin.HyperStackMaker` | `` | `JavaBridge` |
| `hyperstacks02` | Image/Hyperstack | `Stack to Hyperstack...` | `` | `ij.plugin.HyperStackConverter` | `stacktohs` | `JavaBridge` |
| `hyperstacks03` | Image/Hyperstack | `Hyperstack to Stack` | `` | `ij.plugin.HyperStackConverter` | `hstostack` | `JavaBridge` |
| `hyperstacks04` | Image/Hyperstack | `Reduce Dimensionality...` | `` | `ij.plugin.HyperStackReducer` | `` | `JavaBridge` |
| `hyperstacks05` | Image/Hyperstack | `Make Subset...` | `` | `ij.plugin.SubstackMaker` | `` | `JavaBridge` |
| `transform01` | Image/Transfor | `Flip Horizontally` | `` | `ij.plugin.filter.Transformer` | `fliph` | `JavaBridge` |
| `transform02` | Image/Transfor | `Flip Vertically` | `` | `ij.plugin.filter.Transformer` | `flipv` | `JavaBridge` |
| `transform03` | Image/Transfor | `Flip Z` | `` | `ij.plugin.StackReverser` | `` | `JavaBridge` |
| `transform04` | Image/Transfor | `Rotate 90 Degrees Right` | `` | `ij.plugin.filter.Transformer` | `right` | `JavaBridge` |
| `transform05` | Image/Transfor | `Rotate 90 Degrees Left` | `` | `ij.plugin.filter.Transformer` | `left` | `JavaBridge` |
| `transform06` | Image/Transfor | `Rotate... ` | `` | `ij.plugin.filter.Rotator` | `` | `JavaBridge` |
| `transform07` | Image/Transfor | `Translate...` | `` | `ij.plugin.filter.Translator` | `` | `JavaBridge` |
| `transform08` | Image/Transfor | `Bin...` | `` | `ij.plugin.Binner` | `` | `JavaBridge` |
| `transform10` | Image/Transfor | `Image to Results` | `` | `ij.plugin.SimpleCommands` | `itor` | `JavaBridge` |
| `transform11` | Image/Transfor | `Results to Image` | `` | `ij.plugin.SimpleCommands` | `rtoi` | `JavaBridge` |
| `zoom01` | Image/Zoo | `In ` | `+` | `ij.plugin.Zoom` | `in` | `JavaBridge` |
| `zoom02` | Image/Zoo | `Out ` | `-` | `ij.plugin.Zoom` | `out` | `JavaBridge` |
| `zoom03` | Image/Zoo | `Original Scale` | `4` | `ij.plugin.Zoom` | `orig` | `JavaBridge` |
| `zoom04` | Image/Zoo | `View 100%` | `5` | `ij.plugin.Zoom` | `100%` | `JavaBridge` |
| `zoom05` | Image/Zoo | `To Selection` | `` | `ij.plugin.Zoom` | `to` | `JavaBridge` |
| `zoom06` | Image/Zoo | `Scale to Fit` | `` | `ij.plugin.Zoom` | `scale` | `JavaBridge` |
| `zoom07` | Image/Zoo | `Set... ` | `` | `ij.plugin.Zoom` | `set` | `JavaBridge` |
| `zoom08` | Image/Zoo | `Maximize` | `` | `ij.plugin.Zoom` | `max` | `JavaBridge` |
| `overlay01` | Image/Overlay | `Add Selection...` | `b` | `ij.plugin.OverlayCommands` | `add` | `JavaBridge` |
| `overlay02` | Image/Overlay | `Add Image...` | `` | `ij.plugin.OverlayCommands` | `image` | `JavaBridge` |
| `overlay03` | Image/Overlay | `Hide Overlay` | `` | `ij.plugin.OverlayCommands` | `hide` | `JavaBridge` |
| `overlay04` | Image/Overlay | `Show Overlay` | `` | `ij.plugin.OverlayCommands` | `show` | `JavaBridge` |
| `overlay05` | Image/Overlay | `From ROI Manager` | `` | `ij.plugin.OverlayCommands` | `from` | `JavaBridge` |
| `overlay06` | Image/Overlay | `To ROI Manager` | `` | `ij.plugin.OverlayCommands` | `to` | `JavaBridge` |
| `overlay07` | Image/Overlay | `Remove Overlay` | `` | `ij.plugin.OverlayCommands` | `remove` | `JavaBridge` |
| `overlay08` | Image/Overlay | `Toggle Overlay` | `` | `ij.plugin.OverlayCommands` | `toggle` | `JavaBridge` |
| `overlay09` | Image/Overlay | `List Elements` | `` | `ij.plugin.OverlayCommands` | `list` | `JavaBridge` |
| `overlay10` | Image/Overlay | `Flatten` | `F` | `ij.plugin.OverlayCommands` | `flatten` | `JavaBridge` |
| `overlay11` | Image/Overlay | `Labels...` | `` | `ij.plugin.OverlayLabels` | `` | `JavaBridge` |
| `overlay12` | Image/Overlay | `Measure Overlay` | `` | `ij.plugin.OverlayCommands` | `measure` | `JavaBridge` |
| `overlay13` | Image/Overlay | `Overlay Options...` | `Y` | `ij.plugin.OverlayCommands` | `options` | `JavaBridge` |
| `lookup01` | Image/Lookup Tabl | `Invert LUT` | `` | `ij.plugin.LutLoader` | `invert` | `JavaBridge` |
| `lookup02` | Image/Lookup Tabl | `Apply LUT` | `` | `ij.plugin.filter.LutApplier` | `` | `JavaBridge` |
| `lookup04` | Image/Lookup Tabl | `Fire` | `` | `ij.plugin.LutLoader` | `fire` | `JavaBridge` |
| `lookup05` | Image/Lookup Tabl | `Grays` | `` | `ij.plugin.LutLoader` | `grays` | `JavaBridge` |
| `lookup06` | Image/Lookup Tabl | `Ice` | `` | `ij.plugin.LutLoader` | `ice` | `JavaBridge` |
| `lookup07` | Image/Lookup Tabl | `Spectrum` | `` | `ij.plugin.LutLoader` | `spectrum` | `JavaBridge` |
| `lookup08` | Image/Lookup Tabl | `3-3-2 RGB` | `` | `ij.plugin.LutLoader` | `3-3-2 RGB` | `JavaBridge` |
| `lookup09` | Image/Lookup Tabl | `Red` | `` | `ij.plugin.LutLoader` | `red` | `JavaBridge` |
| `lookup10` | Image/Lookup Tabl | `Green` | `` | `ij.plugin.LutLoader` | `green` | `JavaBridge` |
| `lookup11` | Image/Lookup Tabl | `Blue` | `` | `ij.plugin.LutLoader` | `blue` | `JavaBridge` |
| `lookup12` | Image/Lookup Tabl | `Cyan` | `` | `ij.plugin.LutLoader` | `cyan` | `JavaBridge` |
| `lookup13` | Image/Lookup Tabl | `Magenta` | `` | `ij.plugin.LutLoader` | `magenta` | `JavaBridge` |
| `lookup14` | Image/Lookup Tabl | `Yellow` | `` | `ij.plugin.LutLoader` | `yellow` | `JavaBridge` |
| `lookup15` | Image/Lookup Tabl | `Red/Green` | `` | `ij.plugin.LutLoader` | `redgreen` | `JavaBridge` |
| `noise01` | Image/Lookup Tabl | `Add Noise` | `` | `ij.plugin.filter.Filters` | `add` | `JavaBridge` |
| `noise02` | Image/Lookup Tabl | `Add Specified Noise...` | `` | `ij.plugin.filter.Filters` | `noise` | `JavaBridge` |
| `noise03` | Image/Lookup Tabl | `Salt and Pepper` | `` | `ij.plugin.filter.SaltAndPepper` | `` | `JavaBridge` |
| `noise05` | Image/Lookup Tabl | `Despeckle` | `` | `ij.plugin.filter.RankFilters` | `despeckle` | `JavaBridge` |
| `noise06` | Image/Lookup Tabl | `Remove Outliers...` | `` | `ij.plugin.filter.RankFilters` | `outliers` | `JavaBridge` |
| `noise07` | Image/Lookup Tabl | `Remove NaNs...` | `` | `ij.plugin.filter.RankFilters` | `nan` | `JavaBridge` |
| `shadows01` | Process/Shadow | `North` | `` | `ij.plugin.filter.Shadows` | `north` | `JavaBridge` |
| `shadows02` | Process/Shadow | `Northeast` | `` | `ij.plugin.filter.Shadows` | `northeast` | `JavaBridge` |
| `shadows03` | Process/Shadow | `East` | `` | `ij.plugin.filter.Shadows` | `east` | `JavaBridge` |
| `shadows04` | Process/Shadow | `Southeast` | `` | `ij.plugin.filter.Shadows` | `southeast` | `JavaBridge` |
| `shadows05` | Process/Shadow | `South` | `` | `ij.plugin.filter.Shadows` | `south` | `JavaBridge` |
| `shadows06` | Process/Shadow | `Southwest` | `` | `ij.plugin.filter.Shadows` | `southwest` | `JavaBridge` |
| `shadows07` | Process/Shadow | `West` | `` | `ij.plugin.filter.Shadows` | `west` | `JavaBridge` |
| `shadows08` | Process/Shadow | `Northwest` | `` | `ij.plugin.filter.Shadows` | `northwest` | `JavaBridge` |
| `shadows10` | Process/Shadow | `Shadows Demo` | `` | `ij.plugin.filter.Shadows` | `demo` | `JavaBridge` |
| `binary01` | Process/Binary | `Make Binary` | `` | `ij.plugin.Thresholder` | `` | `JavaBridge` |
| `binary02` | Process/Binary | `Convert to Mask` | `` | `ij.plugin.Thresholder` | `mask` | `JavaBridge` |
| `binary04` | Process/Binary | `Erode` | `` | `ij.plugin.filter.Binary` | `erode` | `JavaBridge` |
| `binary05` | Process/Binary | `Dilate` | `` | `ij.plugin.filter.Binary` | `dilate` | `JavaBridge` |
| `binary06` | Process/Binary | `Open` | `` | `ij.plugin.filter.Binary` | `open` | `JavaBridge` |
| `binary07` | Process/Binary | `Close-` | `` | `ij.plugin.filter.Binary` | `close` | `JavaBridge` |
| `binary08` | Process/Binary | `Median` | `` | `ij.plugin.filter.Binary` | `median` | `JavaBridge` |
| `binary10` | Process/Binary | `Outline` | `` | `ij.plugin.filter.Binary` | `outline` | `JavaBridge` |
| `binary11` | Process/Binary | `Fill Holes` | `` | `ij.plugin.filter.Binary` | `fill` | `JavaBridge` |
| `binary12` | Process/Binary | `Skeletonize` | `` | `ij.plugin.filter.Binary` | `skel` | `JavaBridge` |
| `binary14` | Process/Binary | `Distance Map` | `` | `ij.plugin.filter.EDM` | `edm` | `JavaBridge` |
| `binary15` | Process/Binary | `Ultimate Points` | `` | `ij.plugin.filter.EDM` | `points` | `JavaBridge` |
| `binary16` | Process/Binary | `Watershed` | `` | `ij.plugin.filter.EDM` | `watershed` | `JavaBridge` |
| `binary17` | Process/Binary | `Voronoi` | `` | `ij.plugin.filter.EDM` | `voronoi` | `JavaBridge` |
| `binary18` | Process/Binary | `Dither...` | `` | `ij.plugin.filter.Dithering` | `` | `JavaBridge` |
| `binary20` | Process/Binary | `Options...` | `` | `ij.plugin.filter.Binary` | `options` | `JavaBridge` |
| `math01` | Process/Math | `Add...` | `` | `ij.plugin.filter.ImageMath` | `add` | `JavaBridge` |
| `math02` | Process/Math | `Subtract...` | `` | `ij.plugin.filter.ImageMath` | `sub` | `JavaBridge` |
| `math03` | Process/Math | `Multiply...` | `` | `ij.plugin.filter.ImageMath` | `mul` | `JavaBridge` |
| `math04` | Process/Math | `Divide...` | `` | `ij.plugin.filter.ImageMath` | `div` | `JavaBridge` |
| `math05` | Process/Math | `AND...` | `` | `ij.plugin.filter.ImageMath` | `and` | `JavaBridge` |
| `math06` | Process/Math | `OR...` | `` | `ij.plugin.filter.ImageMath` | `or` | `JavaBridge` |
| `math07` | Process/Math | `XOR...` | `` | `ij.plugin.filter.ImageMath` | `xor` | `JavaBridge` |
| `math08` | Process/Math | `Min...` | `` | `ij.plugin.filter.ImageMath` | `min` | `JavaBridge` |
| `math09` | Process/Math | `Max...` | `` | `ij.plugin.filter.ImageMath` | `max` | `JavaBridge` |
| `math10` | Process/Math | `Gamma...` | `` | `ij.plugin.filter.ImageMath` | `gamma` | `JavaBridge` |
| `math11` | Process/Math | `Set...` | `` | `ij.plugin.filter.ImageMath` | `set` | `JavaBridge` |
| `math12` | Process/Math | `Log` | `` | `ij.plugin.filter.ImageMath` | `log` | `JavaBridge` |
| `math13` | Process/Math | `Exp` | `` | `ij.plugin.filter.ImageMath` | `exp` | `JavaBridge` |
| `math14` | Process/Math | `Square` | `` | `ij.plugin.filter.ImageMath` | `sqr` | `JavaBridge` |
| `math15` | Process/Math | `Square Root` | `` | `ij.plugin.filter.ImageMath` | `sqrt` | `JavaBridge` |
| `math16` | Process/Math | `Reciprocal` | `` | `ij.plugin.filter.ImageMath` | `reciprocal` | `JavaBridge` |
| `math17` | Process/Math | `NaN Background` | `` | `ij.plugin.filter.ImageMath` | `nan` | `JavaBridge` |
| `math18` | Process/Math | `Abs` | `` | `ij.plugin.filter.ImageMath` | `abs` | `JavaBridge` |
| `math19` | Process/Math | `Macro...` | `` | `ij.plugin.filter.ImageMath` | `macro` | `JavaBridge` |
| `fft01` | Process/FFT | `FFT` | `` | `ij.plugin.FFT` | `fft` | `JavaBridge` |
| `fft02` | Process/FFT | `Inverse FFT` | `` | `ij.plugin.FFT` | `inverse` | `JavaBridge` |
| `fft03` | Process/FFT | `Redisplay Power Spectrum` | `` | `ij.plugin.FFT` | `redisplay` | `JavaBridge` |
| `fft04` | Process/FFT | `FFT Options...` | `` | `ij.plugin.FFT` | `options` | `JavaBridge` |
| `fft06` | Process/FFT | `Bandpass Filter...` | `` | `ij.plugin.filter.FFTFilter` | `` | `JavaBridge` |
| `fft07` | Process/FFT | `Custom Filter...` | `` | `ij.plugin.filter.FFTCustomFilter` | `` | `JavaBridge` |
| `fft08` | Process/FFT | `FD Math...` | `` | `ij.plugin.FFTMath` | `` | `JavaBridge` |
| `fft09` | Process/FFT | `Swap Quadrants` | `` | `ij.plugin.FFT` | `swap` | `JavaBridge` |
| `fft10` | Process/FFT | `Make Circular Selection...` | `` | `ij.plugin.CircularRoiMaker` | `` | `JavaBridge` |
| `filters01` | Process/Filter | `Convolve...` | `` | `ij.plugin.filter.Convolver` | `` | `JavaBridge` |
| `filters02` | Process/Filter | `Gaussian Blur...` | `` | `ij.plugin.filter.GaussianBlur` | `` | `JavaBridge` |
| `filters03` | Process/Filter | `Median...` | `` | `ij.plugin.filter.RankFilters` | `median` | `JavaBridge` |
| `filters04` | Process/Filter | `Mean...` | `` | `ij.plugin.filter.RankFilters` | `mean` | `JavaBridge` |
| `filters05` | Process/Filter | `Minimum...` | `` | `ij.plugin.filter.RankFilters` | `min` | `JavaBridge` |
| `filters06` | Process/Filter | `Maximum...` | `` | `ij.plugin.filter.RankFilters` | `max` | `JavaBridge` |
| `filters07` | Process/Filter | `Unsharp Mask...` | `` | `ij.plugin.filter.UnsharpMask` | `` | `JavaBridge` |
| `filters08` | Process/Filter | `Variance...` | `` | `ij.plugin.filter.RankFilters` | `variance` | `JavaBridge` |
| `filters09` | Process/Filter | `Top Hat...` | `` | `ij.plugin.filter.RankFilters` | `tophat` | `JavaBridge` |
| `filters11` | Process/Filter | `Gaussian Blur 3D...` | `` | `ij.plugin.GaussianBlur3D` | `` | `JavaBridge` |
| `filters12` | Process/Filter | `Median 3D...` | `` | `ij.plugin.Filters3D` | `median` | `JavaBridge` |
| `filters13` | Process/Filter | `Mean 3D...` | `` | `ij.plugin.Filters3D` | `mean` | `JavaBridge` |
| `filters14` | Process/Filter | `Minimum 3D...` | `` | `ij.plugin.Filters3D` | `min` | `JavaBridge` |
| `filters15` | Process/Filter | `Maximum 3D...` | `` | `ij.plugin.Filters3D` | `max` | `JavaBridge` |
| `filters16` | Process/Filter | `Variance 3D...` | `` | `ij.plugin.Filters3D` | `var` | `JavaBridge` |
| `filters18` | Process/Filter | `Show Circular Masks...` | `` | `ij.plugin.filter.RankFilters` | `masks` | `JavaBridge` |
| `batch01` | File/Batch | `Measure...` | `` | `ij.plugin.BatchMeasure` | `` | `JavaBridge` |
| `batch02` | File/Batch | `Convert...` | `` | `ij.plugin.BatchConverter` | `` | `JavaBridge` |
| `batch03` | File/Batch | `Macro... ` | `` | `ij.plugin.BatchProcessor` | `` | `JavaBridge` |
| `batch04` | File/Batch | `Virtual Stack...` | `` | `ij.plugin.BatchProcessor` | `stack` | `JavaBridge` |
| `gels01` | Analyze/Gel | `Select First Lane` | `1` | `ij.plugin.GelAnalyzer` | `first` | `JavaBridge` |
| `gels02` | Analyze/Gel | `Select Next Lane` | `2` | `ij.plugin.GelAnalyzer` | `next` | `JavaBridge` |
| `gels03` | Analyze/Gel | `Plot Lanes` | `3` | `ij.plugin.GelAnalyzer` | `plot` | `JavaBridge` |
| `gels04` | Analyze/Gel | `Re-plot Lanes` | `` | `ij.plugin.GelAnalyzer` | `replot` | `JavaBridge` |
| `gels05` | Analyze/Gel | `Reset` | `` | `ij.plugin.GelAnalyzer` | `reset` | `JavaBridge` |
| `gels06` | Analyze/Gel | `Label Peaks` | `` | `ij.plugin.GelAnalyzer` | `label` | `JavaBridge` |
| `gels07` | Analyze/Gel | `Gel Analyzer Options...` | `` | `ij.plugin.GelAnalyzer` | `options` | `JavaBridge` |
| `tools01` | Analyze/Tool | `Save XY Coordinates...` | `` | `ij.plugin.XYCoordinates` | `` | `JavaBridge` |
| `tools02` | Analyze/Tool | `Fractal Box Count...` | `` | `ij.plugin.filter.FractalBoxCounter` | `` | `JavaBridge` |
| `tools03` | Analyze/Tool | `Analyze Line Graph` | `` | `ij.plugin.filter.LineGraphAnalyzer` | `` | `JavaBridge` |
| `tools04` | Analyze/Tool | `Curve Fitting...` | `` | `ij.plugin.frame.Fitter` | `` | `JavaBridge` |
| `tools05` | Analyze/Tool | `ROI Manager...` | `` | `ij.plugin.frame.RoiManager` | `` | `JavaBridge` |
| `tools06` | Analyze/Tool | `Scale Bar...` | `` | `ij.plugin.ScaleBar` | `` | `JavaBridge` |
| `tools07` | Analyze/Tool | `Calibration Bar...` | `` | `ij.plugin.CalibrationBar` | `` | `JavaBridge` |
| `tools08` | Analyze/Tool | `Synchronize Windows` | `` | `ij.plugin.frame.SyncWindows` | `` | `JavaBridge` |
| `tools09` | Analyze/Tool | `Grid...` | `` | `ij.plugin.Grid` | `` | `JavaBridge` |
| `tools10` | Analyze/Tool | `Compare Images...` | `` | `ij.plugin.ImageComparator` | `` | `JavaBridge` |
| `macros01` | Plugins/Macro | `Install...` | `` | `ij.plugin.MacroInstaller` | `` | `JavaBridge` |
| `macros02` | Plugins/Macro | `Run...` | `` | `ij.plugin.Macro_Runner` | `` | `JavaBridge` |
| `macros03` | Plugins/Macro | `Edit...` | `` | `ij.plugin.Compiler` | `edit` | `JavaBridge` |
| `macros04` | Plugins/Macro | `Startup Macros...` | `` | `ij.plugin.Commands` | `startup` | `JavaBridge` |
| `macros05` | Plugins/Macro | `Interactive Interpreter...` | `j` | `ij.plugin.SimpleCommands` | `interactive` | `JavaBridge` |
| `macros06` | Plugins/Macro | `Record...` | `u` | `ij.plugin.frame.Recorder` | `` | `JavaBridge` |
| `shortcuts01` | Plugins/Shortcut | `Add Shortcut... ` | `` | `ij.plugin.Hotkeys` | `install` | `JavaBridge` |
| `shortcuts02` | Plugins/Shortcut | `Add Shortcut by Name... ` | `` | `ij.plugin.Hotkeys` | `install2` | `JavaBridge` |
| `shortcuts03` | Plugins/Shortcut | `Remove Shortcut...` | `` | `ij.plugin.Hotkeys` | `remove` | `JavaBridge` |
| `shortcuts04` | Plugins/Shortcut | `List Shortcuts` | `` | `ij.plugin.CommandLister` | `shortcuts` | `JavaBridge` |
| `shortcuts05` | Plugins/Shortcut | `List Commands` | `` | `ij.plugin.Hotkeys` | `list` | `JavaBridge` |
| `utilities01` | Plugins/Utiliti | `Control Panel...` | `U` | `ij.plugin.ControlPanel` | `` | `JavaBridge` |
| `utilities02` | Plugins/Utiliti | `Find Commands...` | `l` | `ij.plugin.CommandFinder` | `` | `JavaBridge` |
| `utilities03` | Plugins/Utiliti | `Commands...` | `L` | `ij.plugin.frame.Commands` | `` | `JavaBridge` |
| `utilities04` | Plugins/Utiliti | `Search...` | `` | `ij.plugin.SimpleCommands` | `search` | `JavaBridge` |
| `utilities05` | Plugins/Utiliti | `Monitor Events...` | `` | `ij.plugin.EventListener` | `` | `JavaBridge` |
| `utilities06` | Plugins/Utiliti | `Monitor Memory...` | `` | `ij.plugin.frame.MemoryMonitor` | `` | `JavaBridge` |
| `utilities08` | Plugins/Utiliti | `Capture Screen` | `G` | `ij.plugin.ScreenGrabber` | `` | `JavaBridge` |
| `utilities09` | Plugins/Utiliti | `Capture Delayed...` | `` | `ij.plugin.ScreenGrabber` | `delay` | `JavaBridge` |
| `utilities10` | Plugins/Utiliti | `Capture Image` | `` | `ij.plugin.ScreenGrabber` | `image` | `JavaBridge` |
| `utilities12` | Plugins/Utiliti | `ImageJ Properties` | `` | `ij.plugin.JavaProperties` | `` | `JavaBridge` |
| `utilities13` | Plugins/Utiliti | `Threads` | `` | `ij.plugin.ThreadLister` | `` | `JavaBridge` |
| `utilities14` | Plugins/Utiliti | `Benchmark` | `` | `ij.plugin.Benchmark` | `` | `JavaBridge` |
| `utilities15` | Plugins/Utiliti | `Reset...` | `` | `ij.plugin.SimpleCommands` | `reset` | `JavaBridge` |
| `utilities16` | Plugins/Utiliti | `DICOM Metadata...` | `` | `ij.plugin.DicomMetadata` | `` | `JavaBridge` |
| `new_01` | Plugins/New | `Macro` | `` | `ij.plugin.NewPlugin` | `macro` | `JavaBridge` |
| `new_02` | Plugins/New | `Macro Tool` | `` | `ij.plugin.NewPlugin` | `macro-tool` | `JavaBridge` |
| `new_03` | Plugins/New | `JavaScript` | `` | `ij.plugin.NewPlugin` | `javascript` | `JavaBridge` |
| `new_05` | Plugins/New | `Plugin` | `` | `ij.plugin.NewPlugin` | `plugin` | `JavaBridge` |
| `new_06` | Plugins/New | `Plugin Filter` | `` | `ij.plugin.NewPlugin` | `filter` | `JavaBridge` |
| `new_07` | Plugins/New | `Plugin Frame` | `` | `ij.plugin.NewPlugin` | `frame` | `JavaBridge` |
| `new_08` | Plugins/New | `Plugin Tool` | `` | `ij.plugin.NewPlugin` | `plugin-tool` | `JavaBridge` |
| `new_10` | Plugins/New | `Text Window...` | `` | `ij.plugin.NewPlugin` | `text+dialog` | `JavaBridge` |
| `about01` | Help/About | `About This Submenu...` | `` | `ij.plugin.SimpleCommands` | `about` | `JavaBridge` |
| `open01` | Help/About | `AuPbSn 40` | `` | `ij.plugin.URLOpener` | `AuPbSn40.jpg` | `JavaBridge` |
| `open02` | Help/About | `Bat Cochlea Volume` | `` | `ij.plugin.URLOpener` | `bat-cochlea-volume.zip` | `JavaBridge` |
| `open03` | Help/About | `Bat Cochlea Renderings` | `` | `ij.plugin.URLOpener` | `bat-cochlea-renderings.zip` | `JavaBridge` |
| `open04` | Help/About | `Blobs` | `B` | `ij.plugin.URLOpener` | `blobs.gif` | `JavaBridge` |
| `open05` | Help/About | `Boats` | `` | `ij.plugin.URLOpener` | `boats.gif` | `JavaBridge` |
| `open06` | Help/About | `Cardio (RGB DICOM)` | `` | `ij.plugin.URLOpener` | `cardio.dcm.zip` | `JavaBridge` |
| `open07` | Help/About | `Cell Colony` | `` | `ij.plugin.URLOpener` | `Cell_Colony.jpg` | `JavaBridge` |
| `open08` | Help/About | `Clown` | `` | `ij.plugin.URLOpener` | `clown.jpg` | `JavaBridge` |
| `open09` | Help/About | `Confocal Series` | `` | `ij.plugin.URLOpener` | `confocal-series.zip` | `JavaBridge` |
| `open10` | Help/About | `CT (16-bit DICOM)` | `` | `ij.plugin.URLOpener` | `ct.dcm.zip` | `JavaBridge` |
| `open11` | Help/About | `Dot Blot` | `` | `ij.plugin.URLOpener` | `Dot_Blot.jpg` | `JavaBridge` |
| `open12` | Help/About | `Embryos` | `` | `ij.plugin.URLOpener` | `embryos.jpg` | `JavaBridge` |
| `open13` | Help/About | `Fluorescent Cells` | `` | `ij.plugin.URLOpener` | `FluorescentCells.zip` | `JavaBridge` |
| `open14` | Help/About | `Fly Brain` | `` | `ij.plugin.URLOpener` | `flybrain.zip` | `JavaBridge` |
| `open15` | Help/About | `Gel` | `` | `ij.plugin.URLOpener` | `gel.gif` | `JavaBridge` |
| `open16` | Help/About | `HeLa Cells (48-bit RGB)` | `` | `ij.plugin.URLOpener` | `hela-cells.zip` | `JavaBridge` |
| `open17` | Help/About | `Image with Overlay` | `` | `ij.plugin.URLOpener` | `ImageWithOverlay.zip` | `JavaBridge` |
| `open18` | Help/About | `Leaf` | `` | `ij.plugin.URLOpener` | `leaf.jpg` | `JavaBridge` |
| `open19` | Help/About | `Line Graph` | `` | `ij.plugin.URLOpener` | `LineGraph.jpg` | `JavaBridge` |
| `open20` | Help/About | `Mitosis (5D stack)` | `` | `ij.plugin.URLOpener` | `Spindly-GFP.zip` | `JavaBridge` |
| `open21` | Help/About | `MRI Stack` | `` | `ij.plugin.URLOpener` | `mri-stack.zip` | `JavaBridge` |
| `open22` | Help/About | `M51 Galaxy (16-bits)` | `` | `ij.plugin.URLOpener` | `m51.zip` | `JavaBridge` |
| `open23` | Help/About | `Neuron (5 channels)` | `` | `ij.plugin.URLOpener` | `Rat_Hippocampal_Neuron.zip` | `JavaBridge` |
| `open24` | Help/About | `Nile Bend` | `` | `ij.plugin.URLOpener` | `NileBend.jpg` | `JavaBridge` |
| `open25` | Help/About | `Organ of Corti (4D stack)` | `` | `ij.plugin.URLOpener` | `organ-of-corti.zip` | `JavaBridge` |
| `open26` | Help/About | `Particles` | `` | `ij.plugin.URLOpener` | `particles.gif` | `JavaBridge` |
| `open27` | Help/About | `T1 Head (16-bits)` | `` | `ij.plugin.URLOpener` | `t1-head.zip` | `JavaBridge` |
| `open28` | Help/About | `T1 Head Renderings` | `` | `ij.plugin.URLOpener` | `t1-rendering.zip` | `JavaBridge` |
| `open29` | Help/About | `TEM Filter` | `` | `ij.plugin.URLOpener` | `TEM_filter_sample.jpg` | `JavaBridge` |
| `open30` | Help/About | `Tree Rings` | `` | `ij.plugin.URLOpener` | `Tree_Rings.jpg` | `JavaBridge` |
