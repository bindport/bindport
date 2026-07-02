use super::*;

pub(crate) fn print_template_list(
    resolver: &TemplateResolver,
    source: Option<TemplateSource>,
) -> Result<(), TemplateCommandError> {
    let templates = resolver.list(source)?;

    if templates.is_empty() {
        println!("No BindPort templates found.");
        return Ok(());
    }

    for template in templates {
        match template.path.as_ref() {
            Some(path) => println!("{}\t{}\t{}", template.name, template.source, path.display()),
            None => println!("{}\t{}", template.name, template.source),
        }
    }

    Ok(())
}

pub(crate) fn print_template_show(
    resolver: &TemplateResolver,
    name: &str,
    source: Option<TemplateSource>,
) -> Result<(), TemplateCommandError> {
    let template = resolver.resolve(name, source)?;

    println!("template: {}", template.name);
    println!("source: {}", template.source);
    if let Some(path) = template.path.as_ref() {
        println!("path: {}", path.display());
    }
    println!();
    print!("{}", template.contents);
    if !template.contents.ends_with('\n') {
        println!();
    }

    Ok(())
}

pub(crate) fn print_template_export(
    resolver: &TemplateResolver,
    name: &str,
    source: Option<TemplateSource>,
) -> Result<(), TemplateCommandError> {
    let template = resolver.resolve(name, source)?;
    print!("{}", template.contents);

    Ok(())
}
