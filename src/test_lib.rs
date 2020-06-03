#[cfg(test)]
pub static valid_template: &str = "{{#if true}}hello world{{else}}goodbye world{{/if}}";

#[cfg(test)]
pub static valid_template_rendered: &str = "hello world";

#[cfg(test)]
pub static invalid_template: &str = "{{this is not valid handlebars!}}";
