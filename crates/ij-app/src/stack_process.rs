//! ImageJ's "Process Stack?" decision, modelled outside the egui widgets.
//!
//! ImageJ only asks when the active image has more than one slice
//! (`IJ.setupDialog`, original_imagej/ij/IJ.java lines 1247-1303). "No" runs
//! the filter on the current slice and records the `slice` option; "Yes"
//! processes the whole stack and records `stack`; "Cancel" aborts without
//! touching the image. Every operation in this module has a verified
//! whole-stack path in `ij-core`.

use ij_session::{Command, ImageId};

/// The `Binary` `StackOperation`'s session command. The menu commands run
/// ImageJ's static option defaults (Binary.java lines 22-23: iterations 1,
/// count 1) with the black-background pref's 1.54u default `true`
/// (Prefs.java 551-553).
fn binary_command(
    operation: ij_core::binary_morphology::BinaryMorphology,
    whole_stack: bool,
) -> Command {
    match operation {
        ij_core::binary_morphology::BinaryMorphology::Erode => Command::BinaryErode {
            iterations: 1,
            count: 1,
            black_background: true,
            whole_stack,
        },
        ij_core::binary_morphology::BinaryMorphology::Dilate => Command::BinaryDilate {
            iterations: 1,
            count: 1,
            black_background: true,
            whole_stack,
        },
        ij_core::binary_morphology::BinaryMorphology::Open => Command::BinaryOpen {
            iterations: 1,
            count: 1,
            black_background: true,
            whole_stack,
        },
        ij_core::binary_morphology::BinaryMorphology::Close => Command::BinaryClose {
            iterations: 1,
            count: 1,
            black_background: true,
            whole_stack,
        },
    }
}

/// The filter or command that was invoked while a stack was active.
#[derive(Debug, Clone, PartialEq)]
pub enum StackOperation {
    Invert,
    FlipHorizontal,
    FlipVertical,
    Smooth,
    Sharpen,
    FindEdges,
    GaussianBlur { sigma: f64 },
    Median { radius: f64 },
    Minimum { radius: f64 },
    Maximum { radius: f64 },
    Despeckle,
    Math(ij_core::math::MathOperation),
    Binary(ij_core::binary_morphology::BinaryMorphology),
    ImageCalculator(ImageCalculatorOperation),
}

/// The pending Image Calculator parameters for the "Process Stack?" question
/// (IJ.setupDialog asks when image1 is a stack, ImageCalculator.java 161-173).
#[derive(Debug, Clone, PartialEq)]
pub struct ImageCalculatorOperation {
    pub title1: String,
    pub title2: String,
    pub operation: ij_core::math::CalculatorOperation,
    pub create_window: bool,
    pub float_result: bool,
}

impl StackOperation {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Invert => "Invert",
            Self::FlipHorizontal => "Flip Horizontally",
            Self::FlipVertical => "Flip Vertically",
            Self::Smooth => "Smooth",
            Self::Sharpen => "Sharpen",
            Self::FindEdges => "Find Edges",
            Self::GaussianBlur { .. } => "Gaussian Blur",
            Self::Median { .. } => "Median",
            Self::Minimum { .. } => "Minimum",
            Self::Maximum { .. } => "Maximum",
            Self::Despeckle => "Despeckle",
            Self::Math(operation) => operation.name(),
            Self::Binary(operation) => match operation {
                ij_core::binary_morphology::BinaryMorphology::Erode => "Erode",
                ij_core::binary_morphology::BinaryMorphology::Dilate => "Dilate",
                ij_core::binary_morphology::BinaryMorphology::Open => "Open",
                ij_core::binary_morphology::BinaryMorphology::Close => "Close",
            },
            Self::ImageCalculator(_) => "Image Calculator",
        }
    }

    /// Prefix used by `ImageJApp` status and error strings.
    pub fn failure_label(&self) -> &'static str {
        match self {
            Self::Invert => "Invert",
            Self::FlipHorizontal => "Flip Horizontally",
            Self::FlipVertical => "Flip Vertically",
            Self::Smooth => "Smooth",
            Self::Sharpen => "Sharpen",
            Self::FindEdges => "Find Edges",
            Self::GaussianBlur { .. } => "Gaussian Blur",
            Self::Median { .. } => "Median filter",
            Self::Minimum { .. } => "Minimum filter",
            Self::Maximum { .. } => "Maximum filter",
            Self::Despeckle => "Despeckle",
            Self::Math(operation) => operation.name(),
            Self::Binary(operation) => match operation {
                ij_core::binary_morphology::BinaryMorphology::Erode => "Erode",
                ij_core::binary_morphology::BinaryMorphology::Dilate => "Dilate",
                ij_core::binary_morphology::BinaryMorphology::Open => "Open",
                ij_core::binary_morphology::BinaryMorphology::Close => "Close",
            },
            Self::ImageCalculator(_) => "Image Calculator",
        }
    }

    /// True when `ij-core` has a verified whole-stack implementation.
    pub fn supports_whole_stack(&self) -> bool {
        matches!(
            self,
            Self::Invert
                | Self::FlipHorizontal
                | Self::FlipVertical
                | Self::Smooth
                | Self::Sharpen
                | Self::FindEdges
                | Self::GaussianBlur { .. }
                | Self::Median { .. }
                | Self::Minimum { .. }
                | Self::Maximum { .. }
                | Self::Despeckle
                | Self::Math(_)
                | Self::Binary(_)
                | Self::ImageCalculator(_)
        )
    }

    /// User-facing reason why "Yes" is not offered.
    pub fn whole_stack_limitation(&self) -> Option<String> {
        if self.supports_whole_stack() {
            None
        } else {
            Some(format!(
                "Whole-stack {} is not implemented in ImageJ-rs. Choose \"Current Slice\" to process only the current slice, or Cancel.",
                self.display_name()
            ))
        }
    }

    /// Current-slice command. `Session` records the `slice` option itself when
    /// the document is a stack.
    pub fn current_slice_command(&self) -> Command {
        match self {
            Self::Invert => Command::Invert,
            Self::FlipHorizontal => Command::FlipHorizontal { whole_stack: false },
            Self::FlipVertical => Command::FlipVertical { whole_stack: false },
            Self::Smooth => Command::Smooth,
            Self::Sharpen => Command::Sharpen,
            Self::FindEdges => Command::FindEdges,
            Self::GaussianBlur { sigma } => Command::GaussianBlur { sigma: *sigma },
            Self::Median { radius } => Command::Median { radius: *radius },
            Self::Minimum { radius } => Command::Minimum { radius: *radius },
            Self::Maximum { radius } => Command::Maximum { radius: *radius },
            Self::Despeckle => Command::Despeckle,
            Self::Math(operation) => Command::Math {
                operation: *operation,
                whole_stack: false,
            },
            Self::Binary(operation) => binary_command(*operation, false),
            Self::ImageCalculator(op) => Command::ImageCalculator {
                title1: op.title1.clone(),
                title2: op.title2.clone(),
                operation: op.operation,
                create_window: op.create_window,
                float_result: op.float_result,
                process_stack: false,
            },
        }
    }

    /// Whole-stack command, or `None` when that path is not implemented.
    pub fn whole_stack_command(&self) -> Option<Command> {
        match self {
            Self::Invert => Some(Command::InvertStack),
            Self::FlipHorizontal => Some(Command::FlipHorizontal { whole_stack: true }),
            Self::FlipVertical => Some(Command::FlipVertical { whole_stack: true }),
            Self::Smooth => Some(Command::SmoothStack),
            Self::Sharpen => Some(Command::SharpenStack),
            Self::FindEdges => Some(Command::FindEdgesStack),
            Self::GaussianBlur { sigma } => Some(Command::GaussianBlurStack { sigma: *sigma }),
            Self::Median { radius } => Some(Command::MedianStack { radius: *radius }),
            Self::Minimum { radius } => Some(Command::MinimumStack { radius: *radius }),
            Self::Maximum { radius } => Some(Command::MaximumStack { radius: *radius }),
            Self::Despeckle => Some(Command::DespeckleStack),
            Self::Math(operation) => Some(Command::Math {
                operation: *operation,
                whole_stack: true,
            }),
            Self::Binary(operation) => Some(binary_command(*operation, true)),
            Self::ImageCalculator(op) => Some(Command::ImageCalculator {
                title1: op.title1.clone(),
                title2: op.title2.clone(),
                operation: op.operation,
                create_window: op.create_window,
                float_result: op.float_result,
                process_stack: true,
            }),
        }
    }

    pub fn success_message(&self) -> String {
        match self {
            Self::Invert => "Inverted active image".to_string(),
            Self::FlipHorizontal => "Flipped image horizontally".to_string(),
            Self::FlipVertical => "Flipped image vertically".to_string(),
            Self::Smooth => "Smooth applied".to_string(),
            Self::Sharpen => "Sharpen applied".to_string(),
            Self::FindEdges => "Find Edges applied".to_string(),
            Self::GaussianBlur { sigma } => {
                format!("Gaussian Blur (sigma={sigma}) applied")
            }
            Self::Median { radius } => format!("Median filter (radius={radius}) applied"),
            Self::Minimum { radius } => format!("Minimum filter (radius={radius}) applied"),
            Self::Maximum { radius } => format!("Maximum filter (radius={radius}) applied"),
            Self::Despeckle => "Despeckle applied".to_string(),
            Self::Math(operation) => format!("{} applied", operation.name()),
            Self::Binary(operation) => {
                let name = match operation {
                    ij_core::binary_morphology::BinaryMorphology::Erode => "Erode",
                    ij_core::binary_morphology::BinaryMorphology::Dilate => "Dilate",
                    ij_core::binary_morphology::BinaryMorphology::Open => "Open",
                    ij_core::binary_morphology::BinaryMorphology::Close => "Close",
                };
                format!("{name} applied")
            }
            Self::ImageCalculator(op) => {
                format!("Image Calculator: {} applied", op.operation.java_name())
            }
        }
    }

    pub fn whole_stack_success_message(&self, stack_size: usize) -> String {
        match self {
            Self::Invert => format!("Inverted all {stack_size} slices"),
            _ => format!("{} to all {stack_size} slices", self.success_message()),
        }
    }
}

/// The pending "Process all N images?" question.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingStackProcess {
    pub image_id: ImageId,
    pub operation: StackOperation,
    pub stack_size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackProcessChoice {
    All,
    Current,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackProcessOutcome {
    Applied,
    /// The user asked for the whole stack, but that path is not implemented.
    /// The pending question stays open so the user can choose a supported path.
    UnsupportedWholeStack,
    Cancelled,
    /// The active image or stack changed while the question was open, so the
    /// pending answer was discarded without touching any document.
    Stale,
    NothingPending,
}
