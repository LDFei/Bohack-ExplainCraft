# 演示媒体来源

图片通过 Unsplash 图片服务获取，下载后随前端本地提供。下面的链接对应实际下载资源；文件名只用于本项目的演示主题分类。

| 本地文件            | 来源                                                                           |
| ------------------- | ------------------------------------------------------------------------------ |
| coffee-shop.jpg     | [Unsplash image](https://images.unsplash.com/photo-1501339847302-ac426a4a7cbb) |
| coffee-cup.jpg      | [Unsplash image](https://images.unsplash.com/photo-1509042239860-f550ce710b93) |
| coffee-brew.jpg     | [Unsplash image](https://images.unsplash.com/photo-1495474472287-4d71bcdd2085) |
| coffee-detail.jpg   | [Unsplash image](https://images.unsplash.com/photo-1442512595331-e89e73853f31) |
| travel-mountain.jpg | [Unsplash image](https://images.unsplash.com/photo-1464822759023-fed622ff2c3b) |
| travel-lake.jpg     | [Unsplash image](https://images.unsplash.com/photo-1470770841072-f978cf4d019e) |
| travel-road.jpg     | [Unsplash image](https://images.unsplash.com/photo-1469474968028-56623f02e42e) |
| product-bottle.jpg  | [Unsplash image](https://images.unsplash.com/photo-1602143407151-7111542de6e8) |

ambient.wav 是用正弦波和包络自行合成的背景音，不含人声、商业录音或其他音乐作品的采样。

九个 MP4 文件由上述本地图片与自行合成的背景音编排生成，用于展示播放器、比例和下载能力，不是调用 AI 生成的视频。可使用已安装的 FFmpeg 运行
`python3 scripts/create-demo-media.py` 重建。
