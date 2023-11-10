# PDMS数据库文件的说明

##### 一、PageType 的说明

1. ​	5 ：Index Page，索引参考号的page信息，根据page信息和offset信息，可以算出具体的位置

     0 级表：总表的索引 

2. ​    3： Session Page，会话页面的信息，包含了上一个senssion page

3. ​     7:   Data page, 存储数据的页面