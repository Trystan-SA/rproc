// The software renderer can't rely on a system font-loading backend the way the
// GPU backends do, so fonts must be baked into the binary at build time. Embed
// resources for the software renderer and compile the Slint entry point.
fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer);
    slint_build::compile_with_config("ui/app.slint", config).expect("compile ui/app.slint");
}
