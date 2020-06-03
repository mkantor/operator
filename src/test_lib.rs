#[cfg(test)]
pub static VALID_TEMPLATE: &str = "{{#if true}}hello world{{else}}goodbye world{{/if}}";

#[cfg(test)]
pub static VALID_TEMPLATE_RENDERED: &str = "hello world";

#[cfg(test)]
pub static INVALID_TEMPLATE: &str = "{{this is not valid handlebars!}}";
