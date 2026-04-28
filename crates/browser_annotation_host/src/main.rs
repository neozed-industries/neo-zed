fn main() -> anyhow::Result<()> {
    browser_annotation_host::run(std::io::stdin().lock(), std::io::stdout().lock())
}
