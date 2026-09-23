// msvc-stdext.h — force-included compatibility shim for Qt 6.5 headers.
//
// qcompilerdetection.h maps Qt's array-iterator helpers to the non-standard
// stdext::make_checked_array_iterator / stdext::make_unchecked_array_iterator
// on every MSVC version. MSVC 19.38 (VS 2022 17.8) deprecated those helpers
// and later toolchains removed them, so Qt 6.5 headers fail to compile with
// C2065/C3861 'stdext'. This header is force-included before every translation
// unit and replaces Qt's macros with the identity fallback that Qt itself uses
// for every other compiler. Newer Qt releases do not need it.
#if defined(_MSC_VER) && _MSC_VER >= 1938
#include <QtCore/qcompilerdetection.h>
#undef QT_MAKE_UNCHECKED_ARRAY_ITERATOR
#define QT_MAKE_UNCHECKED_ARRAY_ITERATOR(x) (x)
#undef QT_MAKE_CHECKED_ARRAY_ITERATOR
#define QT_MAKE_CHECKED_ARRAY_ITERATOR(x, N) (x)
#endif