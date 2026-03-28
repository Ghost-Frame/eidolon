cmake_minimum_required(VERSION 3.16)
project(engram_brain VERSION 1.0.0 LANGUAGES CXX C)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()

add_compile_options(-O3 -march=native -Wall -Wextra -Wno-unused-parameter)

# --- Eigen3 ---
find_package(Eigen3 3.3 QUIET NO_MODULE)
if(NOT Eigen3_FOUND)
  find_path(EIGEN3_INCLUDE_DIR
    NAMES Eigen/Core
    PATHS /usr/include/eigen3 /usr/local/include/eigen3 /usr/local/include
    REQUIRED
  )
  message(STATUS "Eigen3 found at: ${EIGEN3_INCLUDE_DIR}")
  add_library(Eigen3::Eigen INTERFACE IMPORTED)
  target_include_directories(Eigen3::Eigen INTERFACE ${EIGEN3_INCLUDE_DIR})
else()
  message(STATUS "Eigen3 found via find_package")
endif()

# --- nlohmann/json header-only from vendor ---
add_library(nlohmann_json INTERFACE)
target_include_directories(nlohmann_json INTERFACE ${CMAKE_SOURCE_DIR}/vendor)

# --- SQLite3 bundled amalgamation ---
add_library(sqlite3_bundled STATIC vendor/sqlite3/sqlite3.c)
target_include_directories(sqlite3_bundled PUBLIC ${CMAKE_SOURCE_DIR}/vendor/sqlite3)
target_compile_definitions(sqlite3_bundled PRIVATE SQLITE_THREADSAFE=1 SQLITE_ENABLE_FTS5)
find_package(Threads REQUIRED)
target_link_libraries(sqlite3_bundled PRIVATE Threads::Threads ${CMAKE_DL_LIBS})

# --- Project include dirs ---
include_directories(${CMAKE_SOURCE_DIR}/include)

# --- Brain module sources ---
set(BRAIN_SOURCES
  src/pca.cpp
  src/substrate.cpp
  src/graph.cpp
  src/interference.cpp
  src/decay.cpp
  src/absorb.cpp
  src/persistence.cpp
)

# --- Main binary ---
add_executable(engram-brain src/main.cpp ${BRAIN_SOURCES})
target_link_libraries(engram-brain PRIVATE Eigen3::Eigen nlohmann_json sqlite3_bundled Threads::Threads)

# --- Test binary ---
add_executable(brain-tests
  tests/test_substrate.cpp
  tests/test_graph.cpp
  tests/test_decay.cpp
  tests/test_e2e.cpp
  ${BRAIN_SOURCES}
)
target_link_libraries(brain-tests PRIVATE Eigen3::Eigen nlohmann_json sqlite3_bundled Threads::Threads)
