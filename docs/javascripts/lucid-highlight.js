// Lucid-specific syntax highlighting for Python code blocks.
//
// Pygments' Python lexer knows Python's own keywords and builtins, but has
// no idea about the names this specification's own language design adds:
// new keywords (`trait`, `dispatch`, `getter`, ...), new capability traits
// and special types (`Iterator`, `Buffer`, `Self`, ...), new builtin
// functions (`fields`), and the lowercase constants that replace Python's
// capitalized `None`/`True`/`False`. This walks each already-highlighted
// code block's text and wraps those names in the same span classes Pygments
// already uses, so they pick up the current theme's colors with no
// additional CSS.
(() => {
  const KEYWORDS = [
    "final", "let", "type", "trust", "without", "any", "skip", "if_broken",
    "match", "case", "dispatch", "contextmanager", "trait", "override",
    "implement", "getter", "setter", "classvar", "classmethod", "sealed",
    "factory", "construct",
  ];

  const TYPES = [
    "Bytes", "ByteArray", "MemoryView", "Callable", "Eq", "Ord", "Hashable",
    "Sized", "Iterable", "Iterator", "Reversible", "Set", "Container",
    "Collection", "Sequence", "Buffer", "Arguments", "Parameters",
    "SourceLocation", "VarName", "Sentinel", "Self", "DottedPath", "Module",
  ];

  const FUNCTIONS = ["fields"];

  const CONSTANTS = ["none", "true", "false"];

  // Pygments span classes: Keyword, Name.Class, Name.Builtin, Keyword.Constant.
  const GROUPS = [
    [KEYWORDS, "k"],
    [TYPES, "nc"],
    [FUNCTIONS, "nb"],
    [CONSTANTS, "kc"],
  ];

  const CLASS_OF = new Map();
  const ALL_WORDS = [];
  for (const [words, cls] of GROUPS) {
    for (const word of words) {
      CLASS_OF.set(word, cls);
      ALL_WORDS.push(word);
    }
  }

  // Longest-first avoids a shorter alternative winning at the same
  // position when one word happens to prefix another.
  ALL_WORDS.sort((a, b) => b.length - a.length);
  const WORD_PATTERN = new RegExp(`\\b(${ALL_WORDS.join("|")})\\b`, "g");

  // Don't rescan inside a span Pygments already gave a specific meaning:
  // string and comment content shouldn't be recolored just because it
  // happens to contain one of these words as text.
  const SKIP_CLASS = /^(s|s1|s2|sa|sb|sc|sd|se|sh|si|sr|ss|sx|c|c1|cm|cs|cp)$/;

  function highlightBlock(block) {
    if (block.dataset.lucidHighlighted) return;
    block.dataset.lucidHighlighted = "true";

    const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT, null);
    const textNodes = [];
    let node;
    while ((node = walker.nextNode())) {
      const parent = node.parentElement;
      if (parent && SKIP_CLASS.test(parent.className)) continue;
      if (WORD_PATTERN.test(node.nodeValue)) textNodes.push(node);
    }

    for (const textNode of textNodes) {
      const text = textNode.nodeValue;
      const frag = document.createDocumentFragment();
      let lastIndex = 0;
      let match;
      WORD_PATTERN.lastIndex = 0;
      while ((match = WORD_PATTERN.exec(text))) {
        frag.appendChild(document.createTextNode(text.slice(lastIndex, match.index)));
        const span = document.createElement("span");
        span.className = CLASS_OF.get(match[1]);
        span.textContent = match[1];
        frag.appendChild(span);
        lastIndex = match.index + match[1].length;
      }
      frag.appendChild(document.createTextNode(text.slice(lastIndex)));
      textNode.replaceWith(frag);
    }
  }

  function highlightAll() {
    document
      .querySelectorAll("div.language-python pre code, div.highlight pre code")
      .forEach(highlightBlock);
  }

  document.addEventListener("DOMContentLoaded", highlightAll);

  // Zensical's instant-navigation swaps page content without a full
  // reload, so DOMContentLoaded alone would only fire once. Re-run
  // whenever new code blocks appear in the document.
  new MutationObserver(highlightAll).observe(document.body, {
    childList: true,
    subtree: true,
  });
})();
