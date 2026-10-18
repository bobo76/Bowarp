fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon/agent-workspace.ico");

    // Embeds the .exe icon shown by Explorer and the taskbar. Only for native Windows builds.
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/app-icon/agent-workspace.ico");
        resource
            .compile()
            .expect("failed to embed the Windows icon");
    }
}
