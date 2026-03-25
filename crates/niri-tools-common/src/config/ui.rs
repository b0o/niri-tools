#[derive(Debug, Clone, Default)]
pub struct UiConfig {
    pub font: Option<String>,
    pub background_color: Option<String>,
    pub color: Option<String>,
    pub corner_radius: Option<f64>,
    pub modes: ModesUiConfig,
    pub scratchpads: ScratchpadsUiConfig,
}

#[derive(Debug, Clone, Default)]
pub struct ModesUiConfig {
    pub font: Option<String>,
    pub background_color: Option<String>,
    pub color: Option<String>,
    pub corner_radius: Option<f64>,
    pub anchor: Option<String>,
    pub separator: Option<String>,
    pub margin_top: Option<i32>,
    pub margin_right: Option<i32>,
    pub margin_bottom: Option<i32>,
    pub margin_left: Option<i32>,
    pub padding: Option<f64>,
    pub column_padding: Option<f64>,
    pub min_width: Option<f64>,
    pub border_width: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct ScratchpadsUiConfig {
    pub font: Option<String>,
    pub background_color: Option<String>,
    pub color: Option<String>,
    pub corner_radius: Option<f64>,
    pub anchor: Option<String>,
    pub padding: Option<f64>,
}
