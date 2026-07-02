use super::*;

#[derive(Debug, Clone)]
pub struct TemplateResolver {
    project_templates: Option<PathBuf>,
    global_templates: Option<PathBuf>,
}

impl TemplateResolver {
    pub fn new(project_templates: Option<PathBuf>, global_templates: Option<PathBuf>) -> Self {
        Self {
            project_templates,
            global_templates,
        }
    }

    pub fn resolve(
        &self,
        name: &str,
        source: Option<TemplateSource>,
    ) -> Result<ResolvedTemplate, TemplateError> {
        validate_template_name(name)?;

        let sources = match source {
            Some(source) => vec![source],
            None => vec![
                TemplateSource::Project,
                TemplateSource::Global,
                TemplateSource::BuiltIn,
            ],
        };

        for source in sources {
            match self.resolve_from_source(name, source)? {
                Some(template) => return Ok(template),
                None => continue,
            }
        }

        Err(TemplateError::NotFound {
            name: name.to_string(),
            source,
        })
    }

    pub fn list(
        &self,
        source: Option<TemplateSource>,
    ) -> Result<Vec<TemplateSummary>, TemplateError> {
        let mut templates = BTreeMap::<String, TemplateSummary>::new();
        let sources = match source {
            Some(source) => vec![source],
            None => vec![
                TemplateSource::Project,
                TemplateSource::Global,
                TemplateSource::BuiltIn,
            ],
        };

        for source in sources {
            for summary in self.list_source(source)? {
                templates.entry(summary.name.clone()).or_insert(summary);
            }
        }

        Ok(templates.into_values().collect())
    }

    pub(crate) fn resolve_from_source(
        &self,
        name: &str,
        source: TemplateSource,
    ) -> Result<Option<ResolvedTemplate>, TemplateError> {
        match source {
            TemplateSource::Project => {
                self.resolve_from_directory(name, source, self.project_templates.as_deref())
            }
            TemplateSource::Global => {
                self.resolve_from_directory(name, source, self.global_templates.as_deref())
            }
            TemplateSource::BuiltIn => Ok(resolve_built_in(name)),
        }
    }

    pub(crate) fn resolve_from_directory(
        &self,
        name: &str,
        source: TemplateSource,
        directory: Option<&Path>,
    ) -> Result<Option<ResolvedTemplate>, TemplateError> {
        let Some(directory) = directory else {
            return Ok(None);
        };

        let exact = directory.join(name);
        if exact.is_file() {
            return read_template(name, source, exact, Vec::new()).map(Some);
        }

        let j2 = directory.join(format!("{name}.j2"));
        if j2.is_file() {
            return read_template(name, source, j2, Vec::new()).map(Some);
        }

        let matches = wildcard_matches(directory, name)?;
        let Some(path) = matches.first().cloned() else {
            return Ok(None);
        };

        read_template(name, source, path, matches).map(Some)
    }

    pub(crate) fn list_source(
        &self,
        source: TemplateSource,
    ) -> Result<Vec<TemplateSummary>, TemplateError> {
        match source {
            TemplateSource::Project => {
                list_directory_templates(source, self.project_templates.as_deref())
            }
            TemplateSource::Global => {
                list_directory_templates(source, self.global_templates.as_deref())
            }
            TemplateSource::BuiltIn => Ok(built_in_templates()
                .iter()
                .map(|template| TemplateSummary {
                    name: template.name.to_string(),
                    source,
                    path: None,
                })
                .collect()),
        }
    }
}
