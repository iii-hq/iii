FROM eclipse-temurin:21-jdk-alpine
WORKDIR /app
COPY LegacyServer.java .
RUN javac LegacyServer.java
EXPOSE 8080
CMD ["java", "LegacyServer"]
