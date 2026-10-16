"""Check the real app/package graph and simulator architecture settings without building."""
import json
from pathlib import Path
import subprocess
import sys

project = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "PurePoint.xcodeproj/project.pbxproj"
objects = json.loads(subprocess.check_output(["plutil", "-convert", "json", "-o", "-", str(project)]))["objects"]
app = next(value for value in objects.values() if value.get("isa") == "PBXNativeTarget" and value.get("name") == "PurePoint")
products = app["packageProductDependencies"]
product = next(key for key in products if objects[key].get("productName") == "SwiftTerm")
package = objects[objects[product]["package"]]
assert package["repositoryURL"] == "https://github.com/migueldeicaza/SwiftTerm.git"
assert package["requirement"] == {"kind": "exactVersion", "version": "1.11.2"}
project_settings = next(value for value in objects.values() if value.get("isa") == "PBXProject")
assert objects[product]["package"] in project_settings["packageReferences"]
frameworks = next(objects[key] for key in app["buildPhases"] if objects[key]["isa"] == "PBXFrameworksBuildPhase")
assert sum(objects[key].get("productRef") == product for key in frameworks["files"]) == 1
configurations = objects[project_settings["buildConfigurationList"]]["buildConfigurations"]
settings = {objects[key]["name"]: objects[key]["buildSettings"] for key in configurations}
# SPM Debug builds the selected simulator architecture. App and test consumers
# must inherit the same setting rather than importing an arm64 module as x86_64.
assert settings["Debug"].get("ONLY_ACTIVE_ARCH") == "YES", "Debug app/test architecture must match the selected SwiftTerm simulator architecture"
assert settings["Release"].get("ONLY_ACTIVE_ARCH") == "NO", "Release must retain all supported architectures"
for target in (value for value in objects.values() if value.get("isa") == "PBXNativeTarget"):
    for key in objects[target["buildConfigurationList"]]["buildConfigurations"]:
        config = objects[key]
        assert config["buildSettings"].get("ONLY_ACTIVE_ARCH", settings[config["name"]]["ONLY_ACTIVE_ARCH"]) == settings[config["name"]]["ONLY_ACTIVE_ARCH"]
print("Xcode SwiftTerm product/framework graph and app/test architecture checks passed")
