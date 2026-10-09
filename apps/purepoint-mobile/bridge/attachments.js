export function validateImages(images) {
  if (images === undefined) return [];
  if (!Array.isArray(images) || images.length > 4)
    throw new Error("Attach at most four images.");
  let size = 0;
  return images.map((image) => {
    if (
      !image ||
      image.type !== "image" ||
      typeof image.data !== "string" ||
      image.data.length > 700000 ||
      !/^[A-Za-z0-9+/]+={0,2}$/.test(image.data)
    )
      throw new Error("Invalid image attachment.");
    const bytes = Buffer.from(image.data, "base64");
    size += bytes.length;
    if (bytes.toString("base64") !== image.data || size > 512 * 1024)
      throw new Error("Images exceed the 512 KiB attachment limit.");
    const png = bytes
      .subarray(0, 8)
      .equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
    const jpeg = bytes[0] === 255 && bytes[1] === 216 && bytes[2] === 255;
    const gif = ["GIF87a", "GIF89a"].includes(bytes.subarray(0, 6).toString());
    const webp =
      bytes.subarray(0, 4).toString() === "RIFF" &&
      bytes.subarray(8, 12).toString() === "WEBP";
    const formats = {
      "image/png": png,
      "image/jpeg": jpeg,
      "image/gif": gif,
      "image/webp": webp,
    };
    if (!Object.hasOwn(formats, image.mimeType) || !formats[image.mimeType])
      throw new Error("Unsupported or invalid image format.");
    return { type: "image", mimeType: image.mimeType, data: image.data };
  });
}
