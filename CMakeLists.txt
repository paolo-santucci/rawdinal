cmake_minimum_required(VERSION 3.21)
project(rawdinal VERSION 0.1.0 LANGUAGES C)

if(CMAKE_CROSSCOMPILING OR NOT CMAKE_SYSTEM_NAME MATCHES "^(Linux|Darwin|Windows)$")
  message(FATAL_ERROR "Rawdinal requires a native Linux, macOS or Windows build")
endif()
if(WIN32 AND NOT MSVC)
  message(FATAL_ERROR "Rawdinal Windows builds require an MSVC-compatible C compiler")
endif()

include(GNUInstallDirs)
find_program(RAWDINAL_CARGO cargo HINTS "$ENV{HOME}/.cargo/bin" "$ENV{USERPROFILE}/.cargo/bin" REQUIRED)

set(RAWDINAL_INSTALL_DEFAULT OFF)
if(CMAKE_SOURCE_DIR STREQUAL PROJECT_SOURCE_DIR)
  set(RAWDINAL_INSTALL_DEFAULT ON)
endif()
option(RAWDINAL_INSTALL "Install the Rawdinal library and development files" ${RAWDINAL_INSTALL_DEFAULT})

set(RAWDINAL_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo")
set(RAWDINAL_CARGO_TARGET_ARGS)
set(RAWDINAL_CARGO_ENV)
set(RAWDINAL_CARGO_OUTPUT_DIR "${RAWDINAL_TARGET_DIR}/release")
if(WIN32)
  if(MSVC_C_ARCHITECTURE_ID STREQUAL "x64")
    set(RAWDINAL_RUST_TARGET "x86_64-pc-windows-msvc")
  elseif(MSVC_C_ARCHITECTURE_ID STREQUAL "X86")
    set(RAWDINAL_RUST_TARGET "i686-pc-windows-msvc")
  elseif(MSVC_C_ARCHITECTURE_ID STREQUAL "ARM64")
    set(RAWDINAL_RUST_TARGET "aarch64-pc-windows-msvc")
  else()
    message(FATAL_ERROR "Unsupported MSVC architecture: ${MSVC_C_ARCHITECTURE_ID}")
  endif()
elseif(APPLE)
  if(CMAKE_OSX_ARCHITECTURES AND NOT CMAKE_OSX_ARCHITECTURES STREQUAL CMAKE_SYSTEM_PROCESSOR)
    message(FATAL_ERROR "Rawdinal macOS builds require a single native architecture: ${CMAKE_SYSTEM_PROCESSOR}")
  endif()
  if(CMAKE_SYSTEM_PROCESSOR STREQUAL "arm64")
    set(RAWDINAL_RUST_TARGET "aarch64-apple-darwin")
  elseif(CMAKE_SYSTEM_PROCESSOR STREQUAL "x86_64")
    set(RAWDINAL_RUST_TARGET "x86_64-apple-darwin")
  else()
    message(FATAL_ERROR "Unsupported macOS architecture: ${CMAKE_SYSTEM_PROCESSOR}")
  endif()
  if(CMAKE_OSX_DEPLOYMENT_TARGET)
    list(APPEND RAWDINAL_CARGO_ENV "MACOSX_DEPLOYMENT_TARGET=${CMAKE_OSX_DEPLOYMENT_TARGET}")
  endif()
endif()
if(WIN32 OR APPLE)
  set(RAWDINAL_CARGO_TARGET_ARGS --target "${RAWDINAL_RUST_TARGET}")
  set(RAWDINAL_CARGO_OUTPUT_DIR "${RAWDINAL_TARGET_DIR}/${RAWDINAL_RUST_TARGET}/release")
endif()
set(RAWDINAL_ARCHIVE "${RAWDINAL_CARGO_OUTPUT_DIR}/${CMAKE_STATIC_LIBRARY_PREFIX}rawdinal_ffi${CMAKE_STATIC_LIBRARY_SUFFIX}")
file(GLOB_RECURSE RAWDINAL_RUST_SOURCES CONFIGURE_DEPENDS
     "${CMAKE_CURRENT_SOURCE_DIR}/src/*.rs"
     "${CMAKE_CURRENT_SOURCE_DIR}/ffi/src/*.rs")

add_custom_command(
  OUTPUT "${RAWDINAL_ARCHIVE}"
  COMMAND "${CMAKE_COMMAND}" -E env ${RAWDINAL_CARGO_ENV}
          "${RAWDINAL_CARGO}" build --release --offline --locked ${RAWDINAL_CARGO_TARGET_ARGS}
          --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
          --target-dir "${RAWDINAL_TARGET_DIR}" -p rawdinal-ffi
  DEPENDS ${RAWDINAL_RUST_SOURCES}
          "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
          "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.lock"
          "${CMAKE_CURRENT_SOURCE_DIR}/ffi/Cargo.toml"
  WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
  VERBATIM)

add_custom_target(rawdinal_build ALL DEPENDS "${RAWDINAL_ARCHIVE}")
add_library(rawdinal STATIC IMPORTED GLOBAL)
set_target_properties(rawdinal PROPERTIES
  IMPORTED_LOCATION "${RAWDINAL_ARCHIVE}"
  INTERFACE_INCLUDE_DIRECTORIES "${CMAKE_CURRENT_SOURCE_DIR}/include")
add_dependencies(rawdinal rawdinal_build)
if(WIN32)
  target_link_libraries(rawdinal INTERFACE advapi32 bcrypt kernel32 ntdll userenv ws2_32)
else()
  find_package(Threads REQUIRED)
  target_link_libraries(rawdinal INTERFACE Threads::Threads ${CMAKE_DL_LIBS} m)
endif()

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
