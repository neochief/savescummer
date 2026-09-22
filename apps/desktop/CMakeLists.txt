add_library(desktop-ui STATIC
    src/service.cpp src/service.h
    src/presentation.cpp src/presentation.h
    src/mainwindow.cpp src/mainwindow.h
    src/demo.cpp src/demo.h)
target_include_directories(desktop-ui PUBLIC src)
target_link_libraries(desktop-ui PUBLIC Qt6::Widgets Qt6::Network Qt6::Svg)
if(WIN32)
    target_link_libraries(desktop-ui PRIVATE advapi32)
endif()
if(MSVC)
    target_compile_options(desktop-ui PRIVATE /W4 /utf-8)
endif()
qt_add_executable(savescummer-desktop WIN32 src/main.cpp resources.qrc)
if(WIN32)
    configure_file(savescummer-desktop.rc.in ${CMAKE_CURRENT_BINARY_DIR}/SaveScummer.rc @ONLY)
    target_sources(savescummer-desktop PRIVATE ${CMAKE_CURRENT_BINARY_DIR}/SaveScummer.rc)
endif()
set_target_properties(savescummer-desktop PROPERTIES
    OUTPUT_NAME SaveScummer
    PDB_NAME SaveScummer
)
target_link_libraries(savescummer-desktop PRIVATE desktop-ui)
install(TARGETS savescummer-desktop RUNTIME DESTINATION bin)
qt_generate_deploy_app_script(TARGET savescummer-desktop OUTPUT_SCRIPT deploy_script
    NO_UNSUPPORTED_PLATFORM_ERROR)
install(SCRIPT ${deploy_script})
if(BUILD_TESTING)
    find_package(Qt6 REQUIRED COMPONENTS Test)
    qt_add_executable(desktop-tests tests/desktop_test.cpp resources.qrc)
    set_target_properties(desktop-tests PROPERTIES WIN32_EXECUTABLE FALSE)
    target_link_libraries(desktop-tests PRIVATE desktop-ui Qt6::Test)
    target_compile_definitions(desktop-tests PRIVATE
        FIXTURE_DIR="${PROJECT_SOURCE_DIR}/protocol/fixtures"
        SOURCE_DIR="${PROJECT_SOURCE_DIR}"
        SAVESCUMMER_SCREENSHOT_DIR="${CMAKE_BINARY_DIR}/screenshots")
    add_test(NAME desktop COMMAND desktop-tests -platform offscreen
        -o -,txt -o ${CMAKE_CURRENT_BINARY_DIR}/desktop-results.xml,junitxml)
    set_tests_properties(desktop PROPERTIES TIMEOUT 60)
endif()
