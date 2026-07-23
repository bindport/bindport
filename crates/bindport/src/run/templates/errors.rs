#[derive(Debug)]
pub(crate) enum TemplateError {
    Unclosed {
        template: String,
    },
    Unopened {
        template: String,
    },
    UnknownPlaceholder {
        placeholder: String,
        template: String,
    },
    UnavailableSiblingField {
        service: String,
        field: String,
    },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unclosed { template } => {
                write!(f, "unclosed template placeholder in `{template}`")
            }
            Self::Unopened { template } => {
                write!(f, "unmatched `}}` in template `{template}`")
            }
            Self::UnknownPlaceholder {
                placeholder,
                template,
            } => {
                write!(
                    f,
                    "unknown or unavailable template placeholder `{placeholder}` in `{template}`"
                )
            }
            Self::UnavailableSiblingField { service, field } => write!(
                f,
                "sibling service `{service}` has no configured `{field}` value in the startup registry snapshot"
            ),
        }
    }
}

impl std::error::Error for TemplateError {}
