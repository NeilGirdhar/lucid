Modern type specification
=========================

Lucid modernizes how user-defined types are specified. Python spreads this work
across classes, dataclasses, ABCs, protocols, mixins, descriptors, properties,
constructors, metaclasses, and special methods. Lucid replaces that scattered
model with three kinds of type specification: interfaces, traits, and classes.

.. contents:: Table of contents
   :depth: 2
   :local:

Lucid separates user-defined type specification into three kinds.

Definitions are private by default. Public type definitions are marked with
``export`` at the definition or re-export site.

Each kind gets its own document: `Interfaces <interfaces.rst>`_ specify
obligations without storing data or providing bodies. `Traits <traits.rst>`_
provide reusable method bodies without owning state. `Classes <classes.rst>`_
own concrete state, construction, and identity. The rest of this page shows
why Python mixes the three together, and how the three work together once
they're kept separate.

Interfaces, traits, and inheritance
-----------------------------------

Interfaces, traits, and classes work together when a small required core can
support rich reusable behavior. A cache only has to say how to fetch, store, and
report freshness; traits can build higher-level behavior from those obligations:

.. code-block:: python

   interface Cache[=K, =V]:
       def get(self, key: K) -> V | none
       def put(self, key: K, value: V) -> none
       def is_fresh(self, key: K) -> bool

   interface Sized:
       def __len__(self) -> int

   trait CacheLookup[K, V](Cache[K, V]):
       def get_or_put(self, key: K, build: Callable[[], V]) -> V:
           cached = self.get(key)
           if cached is not none and self.is_fresh(key):
               return cached
           value = build()
           self.put(key, value)
           return value

   trait SizedCacheSummary(Sized):
       getter empty(self) -> bool:
           return self.__len__() == 0

   class MemoryCache[K: !Hashable, V](Cache[K, V], Sized, CacheLookup[K, V], SizedCacheSummary):
       entries: dict[K, V] = {:}
       fresh: set[K] = {}

       def get(self, key: K) -> V | none:
           return self.entries.get(key)

       def put(self, key: K, value: V) -> none:
           self.entries[key] = value
           self.fresh.add(key)

       def is_fresh(self, key: K) -> bool:
           return key in self.fresh

       def __len__(self) -> int:
           return len(self.entries)

   cache = MemoryCache[str, User]()
   user = cache.get_or_put("ada", lambda: load_user("ada"))
