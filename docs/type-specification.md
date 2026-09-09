# Modern type specification

Lucid modernizes how user-defined types are specified. Python spreads this work
across classes, dataclasses, ABCs, protocols, mixins, descriptors, properties,
constructors, metaclasses, and special methods. Lucid replaces that scattered
model with two kinds of type specification: traits and classes.

Lucid separates user-defined type specification into two kinds.

Definitions are visible everywhere in the project by default; a leading
`_` makes one private instead — see
[Module-private names](modules.md).

Each kind gets its own document: [Traits](traits.md) specify
obligations, reusable method bodies, or both, without owning state.
[Classes](classes.md) own concrete state, construction, and identity.
The rest of this page shows why Python mixes the two together, and how
they work together once they're kept apart.

## Traits and inheritance

A trait and a class work together when a small required core can support
rich reusable behavior on top of it. A cache only has to say how to fetch,
store, and report freshness; the same declaration can build higher-level
behavior directly on those obligations:

```python
trait Cache[=K, =V]:
    def get(self: ~Self, key: K) -> V | none
    def put(self, key: K, value: V) -> none
    def is_fresh(self: ~Self, key: K) -> bool

    def get_or_put(self, key: K, build: () -> V) -> V:
        cached = self.get(key)
        if cached is not none and self.is_fresh(key):
            return cached
        value = build()
        self.put(key, value)
        return value

trait Sized:
    def __len__(self: ~Self) -> int

    getter empty(self) -> bool:
        return self.__len__() == 0

class MemoryCache[K: !Hashable, V](Cache[K, V], Sized):
    entries: dict[K, V] = {:}
    fresh: set[K] = {}

    def get(self: ~Self, key: K) -> V | none:
        return self.entries.get(key)

    def put(self, key: K, value: V) -> none:
        self.entries[key] = value
        self.fresh.add(key)

    def is_fresh(self: ~Self, key: K) -> bool:
        return key in self.fresh

    def __len__(self: ~Self) -> int:
        return len(self.entries)

cache = MemoryCache[str, User]()
user = cache.get_or_put("ada", def(): load_user("ada"))
```
