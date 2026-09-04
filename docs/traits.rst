Traits
======

.. contents:: Table of contents
   :depth: 3
   :local:

Traits provide reusable behavior. A trait can depend on interface obligations
and provide method bodies in terms of those obligations, but it does not own
stored state or concrete identity.

Traits provide reusable method bodies. They do not declare fields.

.. code-block:: python

   interface Renderable:
       def render(self) -> str

   trait DebugRenderable(Renderable):
       def debug(self) -> str:
           return "<debug " + self.render() + ">"

Trait conflicts are explicit. If two traits define the same method, the class
must resolve the collision.

Multiple inheritance in Python
------------------------------

Python multiple inheritance uses the same base-class list for several different
jobs: *class inheritance*, interface promises, mixin behavior, metaclass
selection, and MRO construction. Those jobs interfere with each other.

Common pitfalls include:

* MRO order changes which implementation a method call reaches
* cooperative ``super()`` only works when every class in the chain follows the
  same calling convention
* base classes can bring incompatible constructor requirements

Cooperative ``super()``
~~~~~~~~~~~~~~~~~~~~~~~

Cooperative multiple inheritance requires every class in the chain to accept
and forward compatible arguments. One class that does not participate breaks
the chain.

.. code-block:: python

   class Audited:
       def save(self, *, audit: bool = True):
           if audit:
               record_audit()
           return super().save(audit=audit)

   class Timestamped:
       def save(self):
           self.updated_at = now()
           return super().save()

   class Document(Audited, Timestamped, BaseDocument):
       pass

   Document().save()

``Audited.save`` forwards ``audit`` to ``Timestamped.save``, but
``Timestamped.save`` does not accept that keyword. The method chain only works
when every participant follows the same forwarding convention.

Incompatible constructors
~~~~~~~~~~~~~~~~~~~~~~~~~

Base classes can require incompatible initialization protocols.

.. code-block:: python

   class FileBacked:
       def __init__(self, path: Path):
           self.path = path

   class NetworkBacked:
       def __init__(self, host: str, port: int):
           self.host = host
           self.port = port

   class Cache(FileBacked, NetworkBacked):
       pass

There is no obvious generated constructor for ``Cache``. One parent needs a
path, the other needs host and port, and neither constructor explains how to
initialize the other base.

Lucid avoids those pitfalls by separating the roles. A class may use
*class inheritance* — inheriting from at most one other class — because a
class is the only one of the three roles that owns stored data. It can
satisfy any number of interfaces because interfaces only declare
obligations. It can use any number of traits because traits provide
reusable bodies without owning state or identity. If traits collide, the
class must resolve the conflict explicitly.

Explicit overrides
--------------------

A method that replaces an inherited implementation — the class's one class
parent's, or a trait's — must be marked ``override``:

.. code-block:: python

   class Timestamped:
       def save(self):
           self.updated_at = now()

   class Document(Timestamped):
       override def save(self):
           super().save()
           write_to_disk(self)

The same rule governs ``override`` as governs variance markers: the checker
warns and offers an autofix while drafting, but the marker must be written
into the source before the API is accepted. That protects against both
directions of the same mistake — a parent or trait gaining a method that
silently starts shadowing an unrelated method of the same name with no
signal anywhere, and an intended override whose name or signature no longer
matches anything, silently becoming an unrelated new method while the
original goes on being called elsewhere.

Because a class has at most one class parent, calling through to the
overridden implementation is unambiguous — ``super()`` always means that one
parent, never a position in an MRO. A linter checks that an ``override``
method calls it, but this check is a suggestion, not a rule: some overrides
exist specifically to replace an inherited implementation entirely, such as
a class resolving a conflict between two traits, and the warning can be
suppressed for those.

Final methods
---------------

``final`` applies to a method the same way it applies to a field or a
class: the same keyword, one relationship fixed permanently — here, that
the method can be overridden at all.

.. code-block:: python

   class Timestamped:
       final def save(self):
           self.updated_at = now()

   class Document(Timestamped):
       override def save(self):  # error: save is final
           ...

``final`` is not valid on an interface member. It protects a method's
implementation from being replaced, and an interface member has no
implementation to protect — there is nothing there yet for ``final`` to
fix in place.

