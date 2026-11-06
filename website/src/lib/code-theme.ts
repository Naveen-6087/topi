/**
 * The Shiki theme for every code block on the site, cut from the page palette: keywords in the payment
 * red, strings and numbers in the proof amber, calls in a soft coral, types in a mint that only code uses.
 */
export const codeTheme = {
  name: "topi-night",
  type: "dark",
  colors: {
    "editor.background": "#161513",
    "editor.foreground": "#e9e7df",
  },
  tokenColors: [
    { scope: ["comment", "punctuation.definition.comment"], settings: { foreground: "#7a746b", fontStyle: "italic" } },
    {
      scope: ["keyword", "storage", "storage.type", "storage.modifier", "keyword.control", "keyword.other", "keyword.operator.new", "keyword.operator.expression"],
      settings: { foreground: "#ff6b4a" },
    },
    { scope: ["string", "string.quoted", "string.template", "punctuation.definition.string"], settings: { foreground: "#f5b83d" } },
    { scope: ["constant.numeric", "constant.language", "constant.character"], settings: { foreground: "#ffd58a" } },
    {
      scope: ["entity.name.function", "support.function", "meta.function-call entity.name.function", "variable.function"],
      settings: { foreground: "#ffa98a" },
    },
    {
      scope: ["entity.name.type", "support.type", "entity.name.class", "support.class", "entity.other.inherited-class", "storage.type.primitive"],
      settings: { foreground: "#6fdcc0" },
    },
    { scope: ["variable.parameter", "variable.other.constant"], settings: { foreground: "#e2c6ff" } },
    { scope: ["keyword.operator", "punctuation", "meta.brace"], settings: { foreground: "#a29d94" } },
  ],
};
