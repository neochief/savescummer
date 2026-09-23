// msvc-stdext.h — force-included compatibility shim for Qt 6.5 headers.
//
// Why this exists
//   Qt 6.5's qcompilerdetection.h maps its array-iterator helpers to the
//   non-standard MSVC extensions stdext::make_checked_array_iterator and
//   stdext::make_unchecked_array_iterator on every MSVC version. Microsoft
//   deprecated those helpers in VS 2022 17.8 (MSVC 19.38) for C++17 and later,
//   and newer toolsets — including the VS 2026 images GitHub Actions runs on —
//   no longer declare them at all. Building Qt 6.5 headers with them fails:
//
//     QtCore\qvarlengtharray.h(...): error C2065: 'stdext': undeclared identifier
//     QtCore\qvarlengtharray.h(...): error C3861: 'stdext': identifier not found
//
// What it does
//   apps/desktop/CMakeLists.txt adds this header with /FI (force include) for
//   MSVC 19.38+, so it runs before any Qt header. It includes
//   QtCore/qcompilerdetection.h first — whose include guard then keeps Qt from
//   redefining the macros — and replaces both macros with the identity
//   fallback Qt itself uses for every non-MSVC compiler. The only loss is
//   MSVC's debug-only checked-iterator bounds checking, matching what GCC and
//   Clang builds already do; the copy/move code paths are unchanged.
//
// When to delete it
//   Once the desktop no longer builds against Qt 6.5 (newer Qt releases
//   dropped these helpers), remove this file and its force include from
//   apps/desktop/CMakeLists.txt. A local build without the shim is the
//   verification.
#if defined(_MSC_VER) && _MSC_VER >= 1938
#include <QtCore/qcompilerdetection.h>
#undef QT_MAKE_UNCHECKED_ARRAY_ITERATOR
#define QT_MAKE_UNCHECKED_ARRAY_ITERATOR(x) (x)
#undef QT_MAKE_CHECKED_ARRAY_ITERATOR
#define QT_MAKE_CHECKED_ARRAY_ITERATOR(x, N) (x)
#endif