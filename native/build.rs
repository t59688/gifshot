fn main() {
    println!("cargo:rerun-if-changed=assets/gifshot.ico");
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/gifshot.ico");
    if let Err(error) = resource.compile() {
        panic!("failed to embed Windows application icon: {error}");
    }
}
