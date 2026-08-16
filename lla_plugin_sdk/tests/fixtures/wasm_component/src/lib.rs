use lla_plugin_sdk::Plugin;

#[derive(Default)]
struct Fixture;

impl Plugin for Fixture {}

lla_plugin_sdk::export_component!(Fixture);
