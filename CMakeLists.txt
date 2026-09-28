cmake_minimum_required(VERSION 3.18)
project(rawdinal VERSION 0.1.0 LANGUAGES C)

if(CMAKE_CROSSCOMPILING OR NOT CMAKE_SYSTEM_NAME STREQUAL "Linux")
  message(FATAL_ERROR "Rawdinal currently supports native Linux builds only")
endif()

include(GNUInstallDirs)
find_program(RAWDINAL_CARGO cargo HINTS "$ENV{HOME}/.cargo/bin" REQUIRED)

set(RAWDINAL_INSTALL_DEFAULT OFF)
if(CMAKE_SOURCE_DIR STREQUAL PROJECT_SOURCE_DIR)
  set(RAWDINAL_INSTALL_DEFAULT ON)
endif()
option(RAWDINAL_INSTALL "Install the Rawdinal library and development files" ${RAWDINAL_INSTALL_DEFAULT})

set(RAWDINAL_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(RAWDINAL_ARCHIVE "${RAWDINAL_TARGET_DIR}/release/librawdinal_ffi.a")
file(GLOB_RECURSE RAWDINAL_RUST_SOURCES CONFIGURE_DEPENDS
     "${CMAKE_CURRENT_SOURCE_DIR}/src/*.rs"
     "${CMAKE_CURRENT_SOURCE_DIR}/ffi/src/*.rs")

add_custom_command(
  OUTPUT "${RAWDINAL_ARCHIVE}"
  COMMAND "${RAWDINAL_CARGO}" build --release --offline --locked
          --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
          --target-dir "${RAWDINAL_TARGET_DIR}" -p rawdinal-ffi
  DEPENDS ${RAWDINAL_RUST_SOURCES}
          "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
          "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.lock"
          "${CMAKE_CURRENT_SOURCE_DIR}/ffi/Cargo.toml"
  VERBATIM)

add_custom_target(rawdinal_build ALL DEPENDS "${RAWDINAL_ARCHIVE}")
add_library(rawdinal STATIC IMPORTED GLOBAL)
set_target_properties(rawdinal PROPERTIES
  IMPORTED_LOCATION "${RAWDINAL_ARCHIVE}"
  INTERFACE_INCLUDE_DIRECTORIES "${CMAKE_CURRENT_SOURCE_DIR}/include")
add_dependencies(rawdinal rawdinal_build)
target_link_libraries(rawdinal INTERFACE ${CMAKE_DL_LIBS} pthread m)

if(CMAKE_SOURCE_DIR STREQUAL PROJECT_SOURCE_DIR)
  include(CTest)
  if(BUILD_TESTING)
    add_executable(rawdinal_sensor_v1_abi_test tests/sensor_v1_abi.c)
    target_compile_features(rawdinal_sensor_v1_abi_test PRIVATE c_std_11)
    target_link_libraries(rawdinal_sensor_v1_abi_test PRIVATE rawdinal)
    add_test(NAME rawdinal_sensor_v1_abi COMMAND rawdinal_sensor_v1_abi_test)
    add_executable(rawdinal_raw_v1_abi_test tests/raw_v1_abi.c)
    target_compile_features(rawdinal_raw_v1_abi_test PRIVATE c_std_11)
    target_link_libraries(rawdinal_raw_v1_abi_test PRIVATE rawdinal)
    add_test(NAME rawdinal_raw_v1_abi COMMAND rawdinal_raw_v1_abi_test)
    add_executable(rawdinal_clipping_v1_abi_test tests/clipping_v1_abi.c)
    target_compile_features(rawdinal_clipping_v1_abi_test PRIVATE c_std_11)
    target_link_libraries(rawdinal_clipping_v1_abi_test PRIVATE rawdinal)
    add_test(NAME rawdinal_clipping_v1_abi COMMAND rawdinal_clipping_v1_abi_test)
  endif()
endif()

if(RAWDINAL_INSTALL)
  install(FILES "${RAWDINAL_ARCHIVE}" DESTINATION "${CMAKE_INSTALL_LIBDIR}")
  install(FILES "${CMAKE_CURRENT_SOURCE_DIR}/include/rawdinal.h"
          DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
  install(FILES
          "${CMAKE_CURRENT_SOURCE_DIR}/LICENSE"
          "${CMAKE_CURRENT_SOURCE_DIR}/THIRD_PARTY_NOTICES"
          DESTINATION "${CMAKE_INSTALL_DATAROOTDIR}/doc/rawdinal")
endif()
