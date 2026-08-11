"""OCR 工具函数"""

from PIL import Image


def ocr_image(image: Image.Image, lang: str = "chi_sim+eng") -> str:
    """对 PIL Image 对象执行 OCR"""
    import pytesseract
    return pytesseract.image_to_string(image, lang=lang)


def ocr_image_path(image_path: str, lang: str = "chi_sim+eng") -> str:
    """对图片文件执行 OCR"""
    import pytesseract
    return pytesseract.image_to_string(Image.open(image_path), lang=lang)