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

  // Pygments already promotes the name right after `class` to Name.Class
  // (`nc`) as part of its Python grammar; it has no idea `trait` or
  // `implement ... for ...` mean the same thing, so those names would
  // otherwise fall through as plain, uncolored identifiers. This walks
  // token-by-token (each token is already its own element, since Pygments
  // wraps every token individually) and promotes the identifier following
  // `trait` or `implement`/`for` the same way, independent of whether that
  // name happens to be in TYPES above — a name declared today by `trait X`
  // gets this without ever adding it to a list by hand.
  const IDENTIFIER = /^[A-Za-z_]\w*$/;

  function promoteTraitNames(block) {
    const tokens = [...block.querySelectorAll("span")].filter(
      (el) => el.children.length === 0,
    );

    const promoteNext = (fromIndex) => {
      const next = tokens
        .slice(fromIndex + 1)
        .find((el) => el.textContent.trim().length > 0);
      if (next && next.className === "n" && IDENTIFIER.test(next.textContent)) {
        next.className = "nc";
      }
    };

    for (let i = 0; i < tokens.length; i++) {
      const text = tokens[i].textContent.trim();
      if (text === "trait") {
        promoteNext(i);
      } else if (text === "implement") {
        // `implement Trait for Type:` — promote both names, but bound the
        // search for `for` to this one statement so an unrelated `for`
        // loop further down the block is never touched.
        promoteNext(i);
        for (let j = i + 1; j < tokens.length; j++) {
          const t = tokens[j].textContent.trim();
          if (t === ":") break;
          if (t === "for") {
            promoteNext(j);
            break;
          }
        }
      }
    }
  }

  function highlightBlock(block) {
    if (block.dataset.lucidHighlighted) return;
    block.dataset.lucidHighlighted = "true";

    promoteTraitNames(block);

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
