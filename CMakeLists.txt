cmake_minimum_required(VERSION 3.21)
# The application version has a single source of truth: the Cargo workspace
# manifest ([workspace.package] version). It is the first line in Cargo.toml
# that starts with a three-part version = "..."; inline dependency versions
# such as foo = { version = "1.2.3" } never match. "\n" pins the match to a
# line start because CMake's ^ only means start-of-file.
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml" _savescummer_manifest)
string(REGEX MATCH "\n[ \t]*version[ \t]*=[ \t]*\"([0-9]+\\.[0-9]+\\.[0-9]+)\"" _savescummer_version "${_savescummer_manifest}")
if(NOT _savescummer_version)
    message(FATAL_ERROR "Cannot read the SaveScummer version from Cargo.toml.")
endif()
project(SaveScummer VERSION ${CMAKE_MATCH_1} LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
include(CTest)
find_package(Qt6 6.5 REQUIRED COMPONENTS Widgets Network Svg)
add_subdirectory(apps/desktop)
